use jix::{
    artifacts, build, lock,
    matrix::{self, MatrixLock},
};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
fn artifact(repo: &Path, name: &str, body: &str) {
    let path = repo.join(format!("t/{name}/1"));
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join(format!("{name}-1.pom")),format!("<project><modelVersion>4.0.0</modelVersion><groupId>t</groupId><artifactId>{name}</artifactId><version>1</version>{body}</project>")).unwrap();
    fs::write(path.join(format!("{name}-1.jar")), name).unwrap();
}
fn declaration(repo: &Path) -> Value {
    json!({"repositories":[format!("file://{}",repo.display())],"environments":{
        "amd64":{"resolutionContext":{"systemProperties":{"os.arch":"amd64"}}},
        "aarch64":{"resolutionContext":{"systemProperties":{"os.arch":"aarch64"}}}
    },"classpaths":{"main":{"dependencies":["t:a:1"]},"runtime":{"scope":"runtime","dependencies":["t:a:1"]}}})
}
fn fixture(repo: &Path) {
    for name in ["x", "y"] {
        artifact(repo, name, "");
    }
    artifact(
        repo,
        "a",
        "<profiles><profile><id>default</id><activation><activeByDefault>true</activeByDefault></activation><dependencies><dependency><groupId>t</groupId><artifactId>x</artifactId><version>1</version></dependency></dependencies></profile><profile><id>arm</id><activation><os><arch>aarch64</arch></os></activation><dependencies><dependency><groupId>t</groupId><artifactId>y</artifactId><version>1</version></dependency></dependencies></profile></profiles>",
    );
}
#[test]
fn contexts_nested_ownership_and_offline_reconciliation() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    fixture(&repo);
    let spec = matrix::normalize(declaration(&repo)).unwrap();
    let locked = matrix::reconcile(&spec, None).unwrap();
    assert_eq!(locked.version, 5);
    let canonical = serde_json::to_value(&locked).unwrap();
    let compact = jix::lock_codec::encode(canonical.clone()).unwrap();
    assert_eq!(compact["kind"], "matrix");
    assert_eq!(jix::lock_codec::decode(compact).unwrap(), canonical);
    assert_eq!(locked.artifacts.len(), 3);
    assert_eq!(locked.context_models.len(), 2);
    for (env, dep) in [("amd64", "x"), ("aarch64", "y")] {
        let view = locked.select(env).unwrap();
        assert_eq!(
            view.resolution.classpaths["main"].artifacts[1].id,
            format!("t:{dep}:jar::1")
        );
        assert_eq!(view.artifacts.len(), 2);
        lock::validate(&view, &view.spec).unwrap();
    }
    fs::remove_dir_all(&repo).unwrap();
    let unchanged = matrix::reconcile(&spec, Some(&locked)).unwrap();
    assert_eq!(
        serde_json::to_value(&locked).unwrap(),
        serde_json::to_value(unchanged).unwrap()
    );
    let mut reduced = spec.clone();
    reduced.environments.remove("amd64");
    let pruned = matrix::reconcile(&reduced, Some(&locked)).unwrap();
    assert_eq!(pruned.artifacts.len(), 2);
    let mut tampered = locked.clone();
    tampered
        .classpath_inputs
        .get_mut("amd64")
        .unwrap()
        .get_mut("main")
        .unwrap()
        .context_hash = Some("0".repeat(64));
    assert!(tampered.validate(&spec).is_err());
    let mut tampered = locked.clone();
    tampered.resolution.environments.insert(
        "unused".into(),
        jix_resolver::ResolutionResult {
            classpaths: Default::default(),
        },
    );
    assert!(tampered.validate(&spec).is_err());
}
#[test]
fn only_changed_pairs_resolve_and_shared_conflicts_fail() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    fixture(&repo);
    artifact(&repo, "b", "");
    let mut value = declaration(&repo);
    value["environments"]["amd64"]["classpaths"] = json!({"main":{"dependencies":["t:b:1"]}});
    let spec = matrix::normalize(value).unwrap();
    let old = matrix::reconcile(&spec, None).unwrap();
    fs::remove_dir_all(repo.join("t/b")).unwrap();
    let mut changed = spec.clone();
    changed
        .environments
        .get_mut("aarch64")
        .unwrap()
        .resolution_context
        .user_properties
        .insert("feature".into(), "on".into());
    let result = matrix::reconcile(&changed, Some(&old)).unwrap();
    assert_eq!(
        serde_json::to_value(&old.classpath_inputs["amd64"]).unwrap(),
        serde_json::to_value(&result.classpath_inputs["amd64"]).unwrap()
    );
    let mut shared = declaration(&repo);
    shared["environments"]["amd64"]["classpaths"] = json!({"main":{"dependencies":["t:a:1"]}});
    let spec = matrix::normalize(shared).unwrap();
    let old = matrix::reconcile(&spec, None).unwrap();
    fs::write(repo.join("t/a/1/a-1.jar"), "changed upstream").unwrap();
    let mut changed = spec.clone();
    changed
        .environments
        .get_mut("aarch64")
        .unwrap()
        .resolution_context
        .user_properties
        .insert("feature".into(), "on".into());
    assert!(matrix::reconcile(&changed, Some(&old)).is_err());
}
#[test]
fn ambiguous_contexts_and_names_are_rejected() {
    let mut value = declaration(Path::new("/tmp/repo"));
    value["resolutionContext"] = json!({});
    assert!(matrix::normalize(value).is_err());
    let mut value = declaration(Path::new("/tmp/repo"));
    value["classpaths"]["main"]["resolutionContext"] = json!({});
    assert!(matrix::normalize(value).is_err());
    let mut value = declaration(Path::new("/tmp/repo"));
    value["environments"]["bad--name"] = json!({"resolutionContext":{}});
    assert!(matrix::normalize(value).is_err());
}
#[test]
fn export_and_verification_require_matching_environment() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    fixture(&repo);
    let locked = matrix::reconcile(&matrix::normalize(declaration(&repo)).unwrap(), None).unwrap();
    let input = tmp.path().join("realized");
    fs::create_dir(&input).unwrap();
    lock::write(&locked, &input.join("jix.lock")).unwrap();
    let view = locked.select("amd64").unwrap();
    let manifest = artifacts::Manifest {
        version: 2,
        selection: Some(locked.selection("amd64").unwrap()),
        artifacts: view
            .provenance
            .keys()
            .map(|p| (p.clone(), repo.join(p)))
            .collect(),
    };
    build::write_json(&input.join("artifacts.json"), &manifest).unwrap();
    artifacts::verify_environment(&locked, &input.join("artifacts.json"), "amd64", None).unwrap();
    assert!(
        artifacts::verify_environment(&locked, &input.join("artifacts.json"), "aarch64", None)
            .is_err()
    );
    let out = tmp.path().join("out");
    assert!(artifacts::materialize_environment(&input, &out, Some("aarch64")).is_err());
    assert!(!out.exists());
    artifacts::materialize(&input, &out).unwrap();
    fs::remove_dir_all(repo).unwrap();
    artifacts::verify_environment(&locked, &out.join("artifacts.json"), "amd64", None).unwrap();
}
#[test]
fn cli_locks_full_matrix_and_switches_single_without_relabeling() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    fixture(&repo);
    let input = tmp.path().join("dependencies.json");
    let output = tmp.path().join("jix.lock");
    build::write_json(&input, &declaration(&repo)).unwrap();
    let run = |args: Vec<&str>| {
        Command::new(env!("CARGO_BIN_EXE_jix"))
            .args(args)
            .output()
            .unwrap()
    };
    let p = run(vec![
        "lock",
        input.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
    ]);
    assert!(p.status.success(), "{}", String::from_utf8_lossy(&p.stderr));
    let locked: MatrixLock =
        serde_json::from_value(jix::lock_codec::read(&output).unwrap()).unwrap();
    let first = Command::new(env!("CARGO_BIN_EXE_jix"))
        .args(["resolve", input.to_str().unwrap()])
        .current_dir(tmp.path())
        .env("JAVA_HOME", "/fake-one")
        .env("OS_ARCH", "amd64")
        .output()
        .unwrap();
    let cwd = tmp.path().join("unrelated");
    fs::create_dir(&cwd).unwrap();
    fs::write(cwd.join("marker"), "not an input").unwrap();
    let second = Command::new(env!("CARGO_BIN_EXE_jix"))
        .args(["resolve", input.to_str().unwrap()])
        .current_dir(&cwd)
        .env("JAVA_HOME", "/fake-two")
        .env("OS_ARCH", "aarch64")
        .output()
        .unwrap();
    assert!(first.status.success() && second.status.success());
    assert_eq!(first.stdout, second.stdout);

    let bytes = fs::read(&output).unwrap();
    let mtime = fs::metadata(&output).unwrap().modified().unwrap();
    assert!(
        run(vec![
            "lock",
            input.to_str().unwrap(),
            "--output",
            output.to_str().unwrap()
        ])
        .status
        .success()
    );
    assert_eq!(fs::read(&output).unwrap(), bytes);
    assert_eq!(fs::metadata(&output).unwrap().modified().unwrap(), mtime);
    let report = run(vec![
        "info",
        output.to_str().unwrap(),
        "--environment",
        "amd64",
        "--json",
    ]);
    assert!(report.status.success());
    let report: Value = serde_json::from_slice(&report.stdout).unwrap();
    assert_eq!(report["lockVersion"], 5);
    assert_eq!(report["environments"].as_object().unwrap().len(), 1);
    assert!(
        !run(vec![
            "lock",
            input.to_str().unwrap(),
            "--environment",
            "amd64"
        ])
        .status
        .success()
    );
    let legacy = locked.select("amd64").unwrap();
    lock::write(&legacy, &output).unwrap();
    fs::remove_dir_all(&repo).unwrap();
    let before = fs::read(&output).unwrap();
    assert!(
        !run(vec![
            "lock",
            input.to_str().unwrap(),
            "--output",
            output.to_str().unwrap()
        ])
        .status
        .success()
    );
    assert_eq!(fs::read(&output).unwrap(), before);
}

#[test]
#[ignore = "requires the pinned Maven 3.9.11 oracle and JDK 21"]
fn matrix_maven_parity() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    for name in ["x", "y", "z"] {
        artifact(&repo, name, "");
    }
    artifact(
        &repo,
        "a",
        "<profiles><profile><id>default</id><activation><activeByDefault>true</activeByDefault></activation><dependencies><dependency><groupId>t</groupId><artifactId>x</artifactId><version>1</version></dependency></dependencies></profile><profile><id>arm</id><activation><os><arch>aarch64</arch></os></activation><dependencies><dependency><groupId>t</groupId><artifactId>y</artifactId><version>1</version></dependency></dependencies></profile><profile><id>new-java</id><activation><jdk>[21,)</jdk></activation><dependencies><dependency><groupId>t</groupId><artifactId>z</artifactId><version>1</version></dependency></dependencies></profile></profiles>",
    );
    // Selection-only oracle JARs are explicitly synthetic, not real binaries.
    for name in ["a", "x", "y", "z"] {
        fs::write(
            repo.join(format!("t/{name}/1/{name}-1.jar")),
            b"PK\x05\x06\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
        )
        .unwrap();
    }
    let mut value = declaration(&repo);
    value["environments"]["amd64"]["resolutionContext"]["systemProperties"]["java.version"] =
        json!("17.0.12");
    value["environments"]["aarch64"]["resolutionContext"]["systemProperties"]["java.version"] =
        json!("21.0.12");
    let spec = matrix::normalize(value).unwrap();
    let result = matrix::resolve(&spec).unwrap();
    assert_ne!(result.environments["amd64"], result.environments["aarch64"]);
    for (name, env) in &spec.environments {
        let request = jix_resolver::DependencySpec {
            repositories: spec.repositories.clone(),
            resolution_context: Some(env.resolution_context.clone()),
            classpaths: env.classpaths.clone(),
        };
        let input = tmp.path().join("oracle.json");
        build::write_json(&input, &request).unwrap();
        let output =
            Command::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/oracle/run.sh"))
                .arg(input)
                .output()
                .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let expected: jix_resolver::ResolutionResult =
            serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result.environments[name], expected, "{name}");
    }
}
