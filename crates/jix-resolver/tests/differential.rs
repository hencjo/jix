mod support;
use jix_resolver::*;
use serde_json::json;
use std::{fs, process::Command};
use support::*;

fn compare(name: &str, f: &Fixture, request: &DependencySpec) {
    let rust = f
        .resolve(request)
        .unwrap_or_else(|e| panic!("{name}: Rust: {e}"));
    let input = f.directory.path().join("request.json");
    fs::write(&input, serde_json::to_vec_pretty(request).unwrap()).unwrap();
    let output = Command::new(root().join("tools/oracle/run.sh"))
        .arg(&input)
        .output()
        .expect("Run tools/oracle/bootstrap.sh first, with Java on PATH");
    assert!(
        output.status.success(),
        "{name}: Java: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let java: ResolutionResult = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|e| panic!("{name}: {e}: {}", String::from_utf8_lossy(&output.stdout)));
    if rust != java {
        let failures = root().join("target/differential-failures");
        fs::create_dir_all(&failures).unwrap();
        let saved = tempfile::Builder::new()
            .prefix("mismatch-")
            .tempdir_in(&failures)
            .unwrap()
            .keep();
        fn copy(source: &std::path::Path, destination: &std::path::Path) {
            fs::create_dir_all(destination).unwrap();
            for entry in fs::read_dir(source).unwrap() {
                let entry = entry.unwrap();
                let target = destination.join(entry.file_name());
                if entry.file_type().unwrap().is_dir() {
                    copy(&entry.path(), &target);
                } else {
                    fs::copy(entry.path(), target).unwrap();
                }
            }
        }
        copy(f.directory.path(), &saved.join("repository"));
        let mut request = request.clone();
        request.repositories = vec![format!("file://{}", saved.join("repository").display())];
        for (file, value) in [
            ("request.json", serde_json::to_value(request).unwrap()),
            ("rust.json", serde_json::to_value(&rust).unwrap()),
            ("java.json", serde_json::to_value(&java).unwrap()),
        ] {
            fs::write(saved.join(file), serde_json::to_vec_pretty(&value).unwrap()).unwrap();
        }
        panic!(
            "Differential mismatch: {name}. Reproducer: {}\nRust: {rust:?}\nJava: {java:?}",
            saved.display()
        );
    }
}

#[test]
#[ignore = "requires bootstrapped Maven 3.9.11 oracle and JDK 21"]
fn focused_maven_parity() {
    let f = Fixture::new();
    for a in ["b", "c", "d", "e"] {
        for v in ["1", "2"] {
            f.pom(a, v, "");
        }
    }
    f.pom(
        "a",
        "1",
        &deps(
            &(dep("b", "1", "")
                + &dep("c", "1", "<scope>runtime</scope>")
                + &dep("d", "1", "<optional>true</optional>")
                + &dep("e", "1", "<scope>provided</scope>")),
        ),
    );
    for scope in [
        ClasspathScope::Compile,
        ClasspathScope::Runtime,
        ClasspathScope::Test,
    ] {
        let mut r = f.request(json!(["fixture:a:1", "fixture:b:2"]));
        r.classpaths.get_mut("main").unwrap().scope = scope;
        compare("nearest/scopes/optional", &f, &r);
    }
    compare(
        "exclusions",
        &f,
        &f.request(json!([{"module":"fixture:a:1","exclude":["fixture:b"]}])),
    );
    f.pom(
        "parent",
        "1",
        &format!(
            "<packaging>pom</packaging><properties><bv>1</bv></properties>{}",
            deps(&dep("b", "${bv}", ""))
        ),
    );
    f.named_pom("fixture","a","1","<parent><groupId>fixture</groupId><artifactId>parent</artifactId><version>1</version></parent><artifactId>a</artifactId><properties><bv>2</bv></properties>");
    compare(
        "inherited properties",
        &f,
        &f.request(json!(["fixture:a:1"])),
    );
    f.pom(
        "bom",
        "1",
        &format!(
            "<packaging>pom</packaging><dependencyManagement>{}</dependencyManagement>",
            deps(&(dep("a", "1", "") + &dep("b", "1", "")))
        ),
    );
    let mut request = f.request(json!(["fixture:a"]));
    request
        .classpaths
        .get_mut("main")
        .unwrap()
        .platforms
        .push("fixture:bom:1".into());
    compare("BOM root management", &f, &request);
    f.pom("a","1",&format!("<profiles><profile><id>default</id><activation><activeByDefault>true</activeByDefault></activation>{}</profile></profiles>",deps(&dep("b","1","<type>test-jar</type>"))));
    compare(
        "default profile and classifier",
        &f,
        &f.request(json!(["fixture:a:1"])),
    );
    f.pom("a","1","<distributionManagement><relocation><artifactId>b</artifactId></relocation></distributionManagement>");
    compare("relocation", &f, &f.request(json!(["fixture:a:1"])));
}

#[test]
#[ignore = "requires bootstrapped Maven 3.9.11 oracle and JDK 21"]
fn generated_dag_parity() {
    // Stable LCG seeds: failures are reproducible, not dependent on a random source.
    for seed in 0..24u64 {
        let f = Fixture::new();
        let mut state = seed + 1;
        for a in (0..6).rev() {
            for version in 1..=2 {
                let mut children = String::new();
                for b in a + 1..6 {
                    state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                    if state >> 61 < 3 {
                        children += &dep(
                            &format!("n{b}"),
                            if state & 16 == 0 { "1" } else { "2" },
                            "",
                        );
                    }
                }
                f.pom(&format!("n{a}"), &version.to_string(), &deps(&children));
            }
        }
        compare(
            &format!("generated seed {seed}"),
            &f,
            &f.request(json!(["fixture:n0:1", "fixture:n1:1"])),
        );
    }
}

#[test]
#[ignore = "requires bootstrapped Maven 3.9.11 oracle and JDK 21"]
fn scope_conflict_parity() {
    let f = Fixture::new();
    f.pom("a", "1", &deps(&dep("c", "1", "<scope>runtime</scope>")));
    f.pom("b", "1", &deps(&dep("c", "1", "")));
    f.pom("c", "1", &deps(&dep("d", "1", "")));
    f.pom("d", "1", "");
    compare(
        "scope widening and propagation",
        &f,
        &f.request(json!(["fixture:a:1", "fixture:b:1"])),
    );
}

#[test]
#[ignore = "requires bootstrapped Maven 3.9.11 oracle and JDK 21"]
fn published_junit_parity() {
    let f = Fixture::new();
    fn copy(source: &std::path::Path, target: &std::path::Path) {
        fs::create_dir_all(target).unwrap();
        for item in fs::read_dir(source).unwrap() {
            let item = item.unwrap();
            let out = target.join(item.file_name());
            if item.file_type().unwrap().is_dir() {
                copy(&item.path(), &out);
            } else {
                fs::copy(item.path(), &out).unwrap();
                if out.extension().is_some_and(|e| e == "pom") {
                    // Placeholder binaries: this test compares published metadata,
                    // not bytecode. These are never published or used for builds.
                    fs::write(
                        out.with_extension("jar"),
                        b"PK\x05\x06\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
                    )
                    .unwrap();
                }
            }
        }
    }
    copy(
        &root().join("fixtures/published/repository"),
        f.directory.path(),
    );
    compare(
        "published JUnit, Reactive Streams and javax.inject",
        &f,
        &f.request(json!([
            "org.junit.jupiter:junit-jupiter-api:5.11.4",
            "org.reactivestreams:reactive-streams:1.0.4",
            "javax.inject:javax.inject:1"
        ])),
    );
}

#[test]
#[ignore = "requires bootstrapped Maven 3.9.11 oracle and JDK 21"]
fn generated_scoped_dag_parity() {
    for seed in 0..32u64 {
        let f = Fixture::new();
        let mut state = seed + 51;
        for a in (0..7).rev() {
            for version in 1..=2 {
                let mut children = String::new();
                for b in a + 1..7 {
                    state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                    if state >> 61 < 4 {
                        let extra = match (state >> 8) % 6 {
                            0 => "<scope>runtime</scope>",
                            1 => "<optional>true</optional>",
                            2 => "<scope>provided</scope>",
                            _ => "",
                        };
                        children += &dep(
                            &format!("n{b}"),
                            if state & 16 == 0 { "1" } else { "2" },
                            extra,
                        );
                    }
                }
                f.pom(&format!("n{a}"), &version.to_string(), &deps(&children));
            }
        }
        let mut request = f.request(json!(["fixture:n0:1", "fixture:n1:1"]));
        if seed % 2 == 0 {
            request.classpaths.get_mut("main").unwrap().scope = ClasspathScope::Runtime;
        }
        compare(&format!("scoped seed {seed}"), &f, &request);
    }
}

#[test]
#[ignore = "requires bootstrapped Maven 3.9.11 oracle and JDK 21"]
fn model_management_parity() {
    let f = Fixture::new();
    for a in ["b", "c", "d"] {
        for v in ["1", "2"] {
            f.pom(a, v, "");
        }
    }
    f.pom(
        "a",
        "1",
        &format!(
            "<properties><skip>true</skip></properties>{}",
            deps(&dep("b", "1", "<optional>${skip}</optional>"))
        ),
    );
    compare(
        "optional interpolation",
        &f,
        &f.request(json!(["fixture:a:1"])),
    );
    f.pom("a", "1", &deps(&dep("b", "1", "")));
    for (name, version) in [("bom1", "1"), ("bom2", "2")] {
        f.pom(
            name,
            "1",
            &format!(
                "<packaging>pom</packaging><dependencyManagement>{}</dependencyManagement>",
                deps(&dep("b", version, ""))
            ),
        );
    }
    let mut request = f.request(json!(["fixture:a:1", "fixture:b"]));
    request.classpaths.get_mut("main").unwrap().platforms =
        vec!["fixture:bom1:1".into(), "fixture:bom2:1".into()];
    compare("first BOM wins", &f, &request);
    f.pom(
        "a",
        "1",
        &format!(
            "<dependencyManagement>{}</dependencyManagement>{}",
            deps(&(dep("bom2", "1", "<type>pom</type><scope>import</scope>") + &dep("b", "1", ""))),
            deps(&dep("b", "", ""))
        ),
    );
    compare(
        "local management beats imported BOM",
        &f,
        &f.request(json!(["fixture:a:1"])),
    );
    f.pom(
        "parent",
        "1",
        &format!(
            "<packaging>pom</packaging>{}",
            deps(&(dep("b", "1", "") + &dep("c", "1", "")))
        ),
    );
    f.pom("a","1", &format!("<parent><groupId>fixture</groupId><artifactId>parent</artifactId><version>1</version></parent>{}",deps(&(dep("c","2","")+&dep("d","1","")))));
    compare(
        "child dependency order",
        &f,
        &f.request(json!(["fixture:a:1"])),
    );
    f.pom("a","1", &format!("<dependencyManagement>{}</dependencyManagement>{}",deps(&dep("b","1","<exclusions><exclusion><groupId>fixture</groupId><artifactId>c</artifactId></exclusion></exclusions>")),deps(&dep("b","",""))));
    f.pom("b", "1", &deps(&dep("c", "1", "")));
    compare("managed exclusions", &f, &f.request(json!(["fixture:a:1"])));
    // A dependency's management must not leak into descendants' explicit versions.
    f.pom(
        "a",
        "1",
        &format!(
            "<dependencyManagement>{}</dependencyManagement>{}",
            deps(&dep("c", "2", "")),
            deps(&dep("b", "1", ""))
        ),
    );
    compare(
        "classic non-transitive dependency management",
        &f,
        &f.request(json!(["fixture:a:1"])),
    );
}

#[test]
#[ignore = "requires bootstrapped Maven 3.9.11 oracle and JDK 21"]
fn effective_model_parity() {
    let f = Fixture::new();
    f.pom("parent","1", &format!("<packaging>pom</packaging><properties><bv>1</bv></properties><dependencyManagement>{}</dependencyManagement>{}",deps(&dep("b","${bv}","<scope>runtime</scope>")),deps(&dep("c","1","<optional>true</optional>"))));
    f.pom(
        "bom",
        "1",
        &format!(
            "<packaging>pom</packaging><dependencyManagement>{}</dependencyManagement>",
            deps(&dep("d", "2", ""))
        ),
    );
    f.pom("a","1", &format!("<parent><groupId>fixture</groupId><artifactId>parent</artifactId><version>1</version></parent><properties><bv>2</bv></properties><dependencyManagement>{}</dependencyManagement>{}",deps(&dep("bom","1","<type>pom</type><scope>import</scope>")),deps(&dep("b","",""))));
    let models = ["fixture:a:1", "fixture:parent:1", "fixture:bom:1"];
    let mut request = serde_json::to_value(f.request(json!([]))).unwrap();
    request["inspectModels"] = json!(models);
    let input = f.directory.path().join("inspect.json");
    fs::write(&input, serde_json::to_vec(&request).unwrap()).unwrap();
    let output = Command::new(root().join("tools/oracle/run.sh"))
        .arg(input)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let mut repository = DirectoryRepository::new(vec![f.directory.path().into()]).unwrap();
    for gav in models {
        let coordinate =
            Dependency::from_spec(&DependencyDeclaration::Coordinate(gav.into())).unwrap();
        let rust = inspect_model(&coordinate, &mut repository).unwrap();
        let java: EffectiveModel = serde_json::from_value(result["models"][gav].clone()).unwrap();
        assert_eq!(rust, java, "effective model: {gav}");
    }
}

#[test]
#[ignore = "requires bootstrapped Maven 3.9.11 oracle and JDK 21"]
fn path_exclusions_and_cycles_parity() {
    let f = Fixture::new();
    f.pom("a", "1", &deps(&dep("x", "1", "")));
    f.pom("b", "1", &deps(&dep("x", "1", "")));
    f.pom("x", "1", &deps(&dep("c", "1", "")));
    f.pom("c", "1", "");
    compare(
        "path-local exclusions",
        &f,
        &f.request(json!([{"module":"fixture:a:1","exclude":["fixture:c"]},"fixture:b:1"])),
    );
    f.pom("x", "1", &deps(&dep("a", "1", "")));
    compare("dependency cycle", &f, &f.request(json!(["fixture:a:1"])));
}

#[test]
#[ignore = "requires bootstrapped Maven 3.9.11 oracle and JDK 21"]
fn repository_origins_and_failures_parity() {
    let f = Fixture::new();
    let fallback = Fixture::new();
    f.pom("a", "1", &deps(&dep("b", "1", "")));
    fallback.pom("b", "1", "");
    let mut request = f.request(json!(["fixture:a:1"]));
    request
        .repositories
        .push(format!("file://{}", fallback.directory.path().display()));
    let mut repository = DirectoryRepository::new(vec![
        f.directory.path().into(),
        fallback.directory.path().into(),
    ])
    .unwrap();
    let rust = resolve(&request, &mut repository).unwrap();
    let mut value = serde_json::to_value(&request).unwrap();
    value["origins"] = json!(true);
    let input = f.directory.path().join("origins.json");
    fs::write(&input, serde_json::to_vec(&value).unwrap()).unwrap();
    let output = Command::new(root().join("tools/oracle/run.sh"))
        .arg(&input)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let java: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(rust, serde_json::from_value(java.clone()).unwrap());
    for (a, origin) in [
        ("a", &request.repositories[0]),
        ("b", &request.repositories[1]),
    ] {
        assert_eq!(
            &repository.origins()[&format!("fixture/{a}/1/{a}-1.pom")],
            origin
        );
        assert_eq!(java["origins"][format!("fixture:{a}:pom::1")], *origin);
    }
    // Strict descriptor policy in both engines: a missing or malformed POM is fatal.
    for (name, body, expected) in [
        ("missing", None, "missing"),
        ("malformed", Some("<project>"), "invalid"),
    ] {
        let f = Fixture::new();
        if let Some(body) = body {
            f.pom("a", "1", "");
            fs::write(f.directory.path().join("fixture/a/1/a-1.pom"), body).unwrap();
        }
        let request = f.request(json!(["fixture:a:1"]));
        let rust = f.resolve(&request).unwrap_err();
        let input = f.directory.path().join("failure.json");
        fs::write(&input, serde_json::to_vec(&request).unwrap()).unwrap();
        let output = Command::new(root().join("tools/oracle/run.sh"))
            .arg(&input)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{name}");
        let java: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            serde_json::to_value(rust.kind).unwrap(),
            json!(expected),
            "{name}"
        );
        assert_eq!(java["error"], expected, "{name}: {java}");
    }
}

#[test]
#[ignore = "requires bootstrapped Maven 3.9.11 oracle and JDK 21"]
fn generated_model_and_optional_parity() {
    for seed in 0..16 {
        let f = Fixture::new();
        f.pom("b", "1", "");
        f.pom("b", "2", "");
        f.pom("parent","1", &format!("<packaging>pom</packaging><properties><v>{}</v></properties><dependencyManagement>{}</dependencyManagement>",seed%2+1,deps(&dep("b","${v}",""))));
        f.pom(
            "bom",
            "1",
            &format!(
                "<packaging>pom</packaging><dependencyManagement>{}</dependencyManagement>",
                deps(&dep("b", if seed % 2 == 0 { "2" } else { "1" }, ""))
            ),
        );
        f.pom("a","1", &format!("<parent><groupId>fixture</groupId><artifactId>parent</artifactId><version>1</version></parent><properties><v>{}</v></properties><dependencyManagement>{}</dependencyManagement>{}",seed%2+1,deps(&dep("bom","1","<type>pom</type><scope>import</scope>")),deps(&dep("b","", if seed%3==0 {"<optional>true</optional>"} else {""}))));
        compare(
            &format!("generated model {seed}"),
            &f,
            &f.request(json!(["fixture:a:1"])),
        );
    }
    for optional in [
        "",
        "<optional>false</optional>",
        "<optional>true</optional>",
    ] {
        let f = Fixture::new();
        f.pom("b", "1", "");
        f.pom("a", "1", &deps(&dep("b", "1", "<optional>true</optional>")));
        f.pom(
            "bom",
            "1",
            &format!(
                "<packaging>pom</packaging><dependencyManagement>{}</dependencyManagement>",
                deps(&dep("b", "1", optional))
            ),
        );
        let mut request = f.request(json!(["fixture:a:1"]));
        request
            .classpaths
            .get_mut("main")
            .unwrap()
            .platforms
            .push("fixture:bom:1".into());
        compare(&format!("sparse optional {optional}"), &f, &request);
    }
}

#[test]
#[ignore = "requires bootstrapped Maven 3.9.11 oracle and JDK 21"]
fn profile_and_relocation_context_parity() {
    let f = Fixture::new();
    for a in ["b", "c", "d"] {
        for v in ["1", "2"] {
            f.pom(a, v, "");
        }
    }
    f.pom("a","1", &format!("{}<profiles><profile><id>one</id><activation><activeByDefault>true</activeByDefault></activation>{}</profile><profile><id>two</id><activation><activeByDefault>true</activeByDefault></activation>{}</profile></profiles>",deps(&(dep("b","1","")+&dep("c","1",""))),deps(&dep("b","2","")),deps(&dep("d","1",""))));
    compare(
        "profile merge and order",
        &f,
        &f.request(json!(["fixture:a:1"])),
    );
    f.pom("a","1", "<distributionManagement><relocation><artifactId>b</artifactId></relocation></distributionManagement>");
    f.pom("b", "1", &deps(&dep("c", "1", "")));
    let mut request = f.request(json!([{"module":"fixture:a:1","exclude":["fixture:c"]}]));
    request.classpaths.insert(
        "other".into(),
        f.request(json!(["fixture:a:1"]))
            .classpaths
            .remove("main")
            .unwrap(),
    );
    compare("relocation cache must not capture exclusions", &f, &request);
}

#[test]
#[ignore = "requires bootstrapped Maven 3.9.11 oracle and JDK 21"]
fn declared_activation_parity() {
    declared_os_and_inheritance_parity();
    let context: ResolutionContext = serde_json::from_value(json!({
        "systemProperties":{"java.version":"21.0.12","os.name":"Linux","os.arch":"amd64","os.version":"6.6","path.separator":":","flag":"system","empty":"","packaging":"war"},
        "userProperties":{"flag":"user"}
    })).unwrap();
    for activation in [
        "<jdk>21</jdk>",
        "<jdk>${wantedJdk}</jdk>",
        "<property><name>${propertyName}</name><value>${propertyValue}</value></property>",
        "<os><name>${wantedOs}</name></os>",
        "<jdk>!1.8</jdk>",
        "<jdk>[17,22)</jdk>",
        "<jdk>[21.0.12,21.0.12)</jdk>",
        "<jdk>[22,)</jdk>",
        "<jdk>[21]</jdk>",
        "<property><name>flag</name><value>user</value></property>",
        "<property><name>flag</name><value>!system</value></property>",
        "<property><name>!flag</name><value>user</value></property>",
        "<property><name>!missing</name></property>",
        "<property><name>empty</name></property>",
        "<property><name>!empty</name></property>",
        "<property><name>pomOnly</name></property>",
        "<os><family>unix</family><name>linux</name><arch>AMD64</arch><version>6.6</version></os>",
        "<os><family>!windows</family></os>",
        "<os><family>windows</family></os>",
        "<file><missing>absent.txt</missing></file>",
        "<file><exists>${basedir}/pom.xml</exists></file>",
        "<jdk>1.8</jdk><property><name>flag</name></property>",
        "<activeByDefault>true</activeByDefault><jdk>1.8</jdk>",
    ] {
        let f = Fixture::new();
        f.pom("b", "1", "");
        f.pom("b", "2", "");
        f.pom("a","1", &format!("<properties><pomOnly>true</pomOnly><wantedJdk>21</wantedJdk><wantedOs>Linux</wantedOs><propertyName>flag</propertyName><propertyValue>user</propertyValue></properties><profiles><profile><id>default</id><activation><activeByDefault>true</activeByDefault></activation>{}</profile><profile><id>conditional</id><activation>{activation}</activation>{}</profile></profiles>", deps(&dep("b","1","")),deps(&dep("b","2",""))));
        let mut request = f.request(json!(["fixture:a:1"]));
        request.resolution_context = Some(context.clone());
        compare(activation, &f, &request);
    }
    let f = Fixture::new();
    f.pom("b", "1", "");
    f.pom("b", "2", "");
    f.pom("a","1",&format!("<properties><flag>1</flag></properties>{}",deps(&(dep("b","${flag}","")+&dep("unused","[4.13.1,)","<scope>test</scope>")+&dep("tools","1","<scope>system</scope><optional>true</optional><systemPath>/not/consulted.jar</systemPath>")))));
    let mut request = f.request(json!(["fixture:a:1"]));
    request.resolution_context = Some(context);
    compare(
        "POM property precedence and unselected metadata",
        &f,
        &request,
    );
}

#[test]
#[ignore = "requires bootstrapped Maven 3.9.11 oracle and JDK 21"]
fn published_environment_profiles_parity() {
    use sha2::{Digest, Sha256};
    let corpus = root().join("fixtures/profile-published");
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(corpus.join("corpus.json")).unwrap()).unwrap();
    let f = Fixture::new();
    for (path, record) in manifest["poms"].as_object().unwrap() {
        assert_eq!(
            record["url"].as_str().unwrap(),
            format!("{}/{path}", manifest["repository"].as_str().unwrap())
        );
        let bytes = fs::read(corpus.join("repository").join(path)).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            record["sha256"].as_str().unwrap()
        );
        let dest = f.directory.path().join(path);
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        fs::write(&dest, bytes).unwrap();
        // Selection tests only: explicitly synthetic, empty JARs, never upstream binaries.
        fs::write(
            dest.with_extension("jar"),
            b"PK\x05\x06\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
        )
        .unwrap();
    }
    let coords = manifest["coordinates"].as_array().unwrap();
    for dependencies in coords
        .iter()
        .map(|c| json!([c]))
        .chain(std::iter::once(json!(coords)))
    {
        let mut request = f.request(dependencies.clone());
        request.resolution_context =
            Some(serde_json::from_value(manifest["resolutionContext"].clone()).unwrap());
        let mut runtime = request.classpaths["main"].clone();
        runtime.scope = ClasspathScope::Runtime;
        request.classpaths.insert("runtime".into(), runtime);
        compare(&format!("published {dependencies}"), &f, &request);
        if dependencies.as_array().unwrap().len() == coords.len() {
            let mut selected = f.resolve(&request).unwrap();
            let compile = selected.classpaths.remove("main").unwrap();
            selected.classpaths.insert("compile".into(), compile);
            let frozen: ResolutionResult =
                serde_json::from_slice(&fs::read(corpus.join("selection.json")).unwrap()).unwrap();
            assert_eq!(selected, frozen, "published combined selection snapshot");
        }
    }
}

fn declared_os_and_inheritance_parity() {
    for (os, separator) in [
        ("Windows 11", ";"),
        ("Mac OS X", ":"),
        ("Darwin", ":"),
        ("Linux", ":"),
    ] {
        let f = Fixture::new();
        for name in ["unix", "windows", "mac", "dos"] {
            f.pom(name, "1", "");
        }
        let mut profiles = String::new();
        for family in ["unix", "windows", "mac", "dos"] {
            profiles.push_str(&format!("<profile><id>{family}</id><activation><os><family>{family}</family></os></activation>{}</profile>",deps(&dep(family,"1",""))));
        }
        f.pom("a", "1", &format!("<profiles>{profiles}</profiles>"));
        let mut request = f.request(json!(["fixture:a:1"]));
        request.resolution_context = Some(
            serde_json::from_value(
                json!({"systemProperties":{"os.name":os,"path.separator":separator}}),
            )
            .unwrap(),
        );
        compare(os, &f, &request);
    }
    let f = Fixture::new();
    f.pom("b", "1", "");
    f.pom("b", "2", "");
    f.pom("parent","1",&format!("<packaging>pom</packaging><profiles><profile><id>one</id><activation><activeByDefault>true</activeByDefault></activation><properties><v>1</v></properties></profile><profile><id>two</id><activation><property><name>switch</name></property></activation><properties><v>2</v></properties></profile></profiles><dependencyManagement>{}</dependencyManagement>",deps(&dep("b","${v}",""))));
    f.pom("bom","1","<parent><groupId>fixture</groupId><artifactId>parent</artifactId><version>1</version></parent><packaging>pom</packaging>");
    f.pom(
        "a",
        "1",
        &format!(
            "<dependencyManagement>{}</dependencyManagement>{}",
            deps(&dep("bom", "1", "<type>pom</type><scope>import</scope>")),
            deps(&dep("b", "", ""))
        ),
    );
    let mut request = f.request(json!(["fixture:a:1"]));
    request.resolution_context = Some(ResolutionContext::default());
    request
        .classpaths
        .get_mut("main")
        .unwrap()
        .platforms
        .push("fixture:bom:1".into());
    let mut other = request.classpaths["main"].clone();
    other.resolution_context =
        Some(serde_json::from_value(json!({"userProperties":{"switch":"yes"}})).unwrap());
    request.classpaths.insert("other".into(), other);
    compare("context isolation through parent and BOM", &f, &request);
    // Inspect each context independently: effective POM metadata is contextual too.
    for (name, cp) in &request.normalized().classpaths {
        let mut inspection = f.request(json!([]));
        inspection.resolution_context = cp.resolution_context.clone();
        let mut input = serde_json::to_value(&inspection).unwrap();
        input["inspectModels"] = json!(["fixture:a:1", "fixture:bom:1", "fixture:parent:1"]);
        let path = f.directory.path().join("inspect-context.json");
        fs::write(&path, serde_json::to_vec(&input).unwrap()).unwrap();
        let output = Command::new(root().join("tools/oracle/run.sh"))
            .arg(path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let java: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let mut repo = DirectoryRepository::new(vec![f.directory.path().into()]).unwrap();
        for gav in ["fixture:a:1", "fixture:bom:1", "fixture:parent:1"] {
            let coordinate =
                Dependency::from_spec(&DependencyDeclaration::Coordinate(gav.into())).unwrap();
            let rust = inspect_model_with_context(
                &coordinate,
                &mut repo,
                cp.resolution_context.as_ref().unwrap(),
            )
            .unwrap();
            let expected: EffectiveModel =
                serde_json::from_value(java["models"][gav].clone()).unwrap();
            assert_eq!(rust, expected, "{name}: {gav}");
        }
    }
}
