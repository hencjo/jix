mod support;
use jix_resolver::*;
use serde_json::json;
use support::*;

#[test]
fn nearest_wins_but_classpath_order_is_preorder() {
    let f = Fixture::new();
    f.pom(
        "a",
        "1",
        &deps(&(dep("deep", "1", "") + &dep("b", "1", ""))),
    );
    f.pom("deep", "1", "");
    f.pom("b", "1", "");
    f.pom("b", "2", "");
    assert_eq!(
        f.ids(&f.request(json!(["fixture:a:1", "fixture:b:2"]))),
        [
            "fixture:a:jar::1",
            "fixture:deep:jar::1",
            "fixture:b:jar::2"
        ]
    );
}
#[test]
fn scopes_optional_and_exclusions() {
    let f = Fixture::new();
    f.pom(
        "a",
        "1",
        &deps(
            &(dep("b", "1", "<scope>runtime</scope>")
                + &dep("c", "1", "<optional>true</optional>")
                + &dep("d", "1", "<scope>test</scope>")),
        ),
    );
    for a in ["b", "c", "d"] {
        f.pom(a, "1", "");
    }
    let mut request = f.request(json!(["fixture:a:1"]));
    assert_eq!(f.ids(&request), ["fixture:a:jar::1"]);
    request.classpaths.get_mut("main").unwrap().scope = ClasspathScope::Runtime;
    assert_eq!(f.ids(&request), ["fixture:a:jar::1", "fixture:b:jar::1"]);
    request = f.request(json!([{"module":"fixture:a:1","exclude":["fixture:*"]}]));
    assert_eq!(f.ids(&request), ["fixture:a:jar::1"]);
}
#[test]
fn bom_parent_and_child_property_override() {
    let f = Fixture::new();
    f.pom(
        "parent",
        "1",
        &format!(
            "<packaging>pom</packaging><properties><bv>1</bv></properties>{}",
            deps(&dep("b", "${bv}", ""))
        ),
    );
    f.named_pom("fixture","a","1","<parent><groupId>fixture</groupId><artifactId>parent</artifactId><version>1</version></parent><artifactId>a</artifactId><properties><bv>2</bv></properties>");
    f.pom("b", "2", "");
    f.pom(
        "bom",
        "1",
        &format!(
            "<packaging>pom</packaging><dependencyManagement>{}</dependencyManagement>",
            deps(&dep("a", "1", ""))
        ),
    );
    let mut request = f.request(json!(["fixture:a"]));
    request
        .classpaths
        .get_mut("main")
        .unwrap()
        .platforms
        .push("fixture:bom:1".into());
    assert_eq!(f.ids(&request), ["fixture:a:jar::1", "fixture:b:jar::2"]);
}
#[test]
fn default_profile_and_classifier() {
    let f = Fixture::new();
    f.pom("a","1", &format!("<profiles><profile><id>default</id><activation><activeByDefault>true</activeByDefault></activation>{}</profile></profiles>",deps(&dep("b","1","<type>test-jar</type>"))));
    f.pom("b", "1", "");
    assert_eq!(
        f.ids(&f.request(json!(["fixture:a:1"]))),
        ["fixture:a:jar::1", "fixture:b:jar:tests:1"]
    );
}
#[test]
fn relocation_and_cycles() {
    let f = Fixture::new();
    f.pom("a","1", "<distributionManagement><relocation><artifactId>b</artifactId></relocation></distributionManagement>");
    f.pom("b", "1", &deps(&dep("b", "1", "")));
    assert_eq!(
        f.ids(&f.request(json!(["fixture:a:1"]))),
        ["fixture:b:jar::1"]
    );
    f.pom("b","1", "<distributionManagement><relocation><artifactId>a</artifactId></relocation></distributionManagement>");
    assert_eq!(
        f.resolve(&f.request(json!(["fixture:a:1"])))
            .unwrap_err()
            .kind,
        ErrorKind::Cycle
    );
}
#[test]
fn rejects_unsupported_and_unsafe_input() {
    let f = Fixture::new();
    for coordinate in ["fixture:a:1-SNAPSHOT", "fixture:a:[1,2)"] {
        assert_eq!(
            f.resolve(&f.request(json!([coordinate]))).unwrap_err().kind,
            ErrorKind::Unsupported
        );
    }
    for coordinate in ["../escape:a:1", "fixture:a:", "fixture:a:1:other"] {
        assert!(f.resolve(&f.request(json!([coordinate]))).is_err());
    }
    f.pom("a","1","<profiles><profile><id>host</id><activation><jdk>21</jdk></activation></profile></profiles>");
    assert_eq!(
        f.resolve(&f.request(json!(["fixture:a:1"])))
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    );
}
#[test]
fn missing_invalid_and_parent_cycle() {
    let f = Fixture::new();
    assert_eq!(
        f.resolve(&f.request(json!(["fixture:a:1"])))
            .unwrap_err()
            .kind,
        ErrorKind::Missing
    );
    f.pom(
        "a",
        "1",
        "<parent><groupId>fixture</groupId><artifactId>a</artifactId><version>1</version></parent>",
    );
    assert_eq!(
        f.resolve(&f.request(json!(["fixture:a:1"])))
            .unwrap_err()
            .kind,
        ErrorKind::Cycle
    );
}
#[test]
fn named_classpaths_are_independent_and_repeatable() {
    let f = Fixture::new();
    f.pom("a", "1", "");
    f.pom("a", "2", "");
    let mut request = f.request(json!(["fixture:a:1"]));
    request.classpaths.insert(
        "other".into(),
        f.request(json!(["fixture:a:2"]))
            .classpaths
            .remove("main")
            .unwrap(),
    );
    let result = f.resolve(&request).unwrap();
    assert_eq!(result, f.resolve(&request).unwrap());
    assert_eq!(
        result.classpaths["main"].artifacts[0].id,
        "fixture:a:jar::1"
    );
    assert_eq!(
        result.classpaths["other"].artifacts[0].id,
        "fixture:a:jar::2"
    );
}
#[test]
fn request_unknown_fields_are_errors() {
    assert!(
        serde_json::from_value::<DependencySpec>(
            json!({"repositories":[],"classpaths":{},"typo":true})
        )
        .is_err()
    );
    let f = Fixture::new();
    let bad = json!({"repositories":[],"classpaths":{"main":{"dependencies":[{"module":"a:b:1","typo":true}]}}});
    assert!(serde_json::from_value::<DependencySpec>(bad).is_err());
    assert!(
        f.resolve(&DependencySpec {
            resolution_context: None,
            repositories: vec![],
            classpaths: Default::default()
        })
        .is_err()
    );
}

#[test]
fn repository_order_and_path_safety() {
    let first = Fixture::new();
    let second = Fixture::new();
    first.pom("a", "1", "<name>first</name>");
    second.pom("a", "1", "<name>second</name>");
    let mut repository = DirectoryRepository::new(vec![
        first.directory.path().into(),
        second.directory.path().into(),
    ])
    .unwrap();
    let bytes = repository.read("fixture/a/1/a-1.pom").unwrap().unwrap();
    assert!(
        String::from_utf8(bytes)
            .unwrap()
            .contains("<name>first</name>")
    );
    assert!(repository.read("../outside").is_err());
    assert!(repository.read("/etc/passwd").is_err());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            second.directory.path(),
            first.directory.path().join("escape"),
        )
        .unwrap();
        assert!(repository.read("escape/fixture/a/1/a-1.pom").is_err());
    }
}

#[test]
fn property_cycle_and_malformed_xml_fail() {
    let f = Fixture::new();
    f.pom(
        "a",
        "1",
        &format!(
            "<properties><one>${{two}}</one><two>${{one}}</two></properties>{}",
            deps(&dep("b", "${one}", ""))
        ),
    );
    assert_eq!(
        f.resolve(&f.request(json!(["fixture:a:1"])))
            .unwrap_err()
            .kind,
        ErrorKind::Cycle
    );
    std::fs::write(f.directory.path().join("fixture/a/1/a-1.pom"), "<project>").unwrap();
    assert_eq!(
        f.resolve(&f.request(json!(["fixture:a:1"])))
            .unwrap_err()
            .kind,
        ErrorKind::Invalid
    );
}
