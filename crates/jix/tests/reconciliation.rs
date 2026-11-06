use jix::{build, lock};
use jix_resolver::DependencySpec;
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

fn artifact(repo: &Path, name: &str, version: &str) {
    let dir = repo.join(format!("t/{name}/{version}"));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(format!("{name}-{version}.pom")), format!("<project><modelVersion>4.0.0</modelVersion><groupId>t</groupId><artifactId>{name}</artifactId><version>{version}</version></project>")).unwrap();
    fs::write(
        dir.join(format!("{name}-{version}.jar")),
        format!("{name}-{version}"),
    )
    .unwrap();
}
fn spec(repo: &Path, cps: Value) -> DependencySpec {
    serde_json::from_value(
        json!({"repositories":[format!("file://{}",repo.display())],"classpaths":cps}),
    )
    .unwrap()
}
fn value(v: &impl serde::Serialize) -> Value {
    serde_json::to_value(v).unwrap()
}

#[test]
fn unchanged_lock_is_offline_and_preserves_bytes_and_mtime() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    artifact(&repo, "a", "1");
    let spec = spec(&repo, json!({"main":{"dependencies":["t:a:1"]}}));
    let old = lock::create(&spec).unwrap();
    assert_eq!(old.version, 5);
    fs::remove_dir_all(&repo).unwrap();
    let new = lock::reconcile(&spec, Some(&old)).unwrap();
    assert_eq!(value(&old), value(&new));
    let input = temp.path().join("dependencies.json");
    build::write_json(&input, &spec).unwrap();
    let output = temp.path().join("jix.lock");
    lock::write(&old, &output).unwrap();
    let bytes = fs::read(&output).unwrap();
    let modified = fs::metadata(&output).unwrap().modified().unwrap();
    let run = Command::new(env!("CARGO_BIN_EXE_jix"))
        .args([
            "lock",
            input.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(fs::read(&output).unwrap(), bytes);
    assert_eq!(fs::metadata(&output).unwrap().modified().unwrap(), modified);
}

#[test]
fn reconcile_prunes_and_preserves_independent_classpaths() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    for (a, v) in [("a", "1"), ("b", "1"), ("b", "2")] {
        artifact(&repo, a, v);
    }
    let initial = spec(
        &repo,
        json!({"main":{"dependencies":["t:a:1"]},"runtime":{"dependencies":["t:b:1"]}}),
    );
    let old = lock::create(&initial).unwrap();
    fs::remove_dir_all(repo.join("t/a")).unwrap();
    fs::remove_dir_all(repo.join("t/b/1")).unwrap();
    let changed = spec(
        &repo,
        json!({"main":{"dependencies":["t:a:1"]},"runtime":{"dependencies":["t:b:2"]}}),
    );
    let new = lock::reconcile(&changed, Some(&old)).unwrap();
    assert_eq!(
        value(&old.resolution.classpaths["main"]),
        value(&new.resolution.classpaths["main"])
    );
    assert_eq!(
        value(&old.classpath_inputs["main"]),
        value(&new.classpath_inputs["main"])
    );
    assert!(!new.provenance.contains_key("t/b/1/b-1.pom"));
    assert!(
        !new.context_models
            .values()
            .any(|m| m.contains_key("t/b/1/b-1.pom"))
    );
    assert!(!new.artifacts.contains_key("t:b:jar::1"));
    let mut removed = changed.clone();
    removed.classpaths.remove("main");
    let pruned = lock::reconcile(&removed, Some(&new)).unwrap();
    assert!(pruned.provenance.keys().all(|p| p.starts_with("t/b/2/")));
    let mut changed_repo = changed.clone();
    changed_repo
        .repositories
        .push(format!("file://{}", temp.path().display()));
    assert!(
        lock::reconcile(&changed_repo, Some(&new)).is_err(),
        "Repository changes must invalidate main too"
    );
    let mut reordered = changed_repo.clone();
    reordered.repositories.reverse();
    let old_multi = {
        let mut l = new.clone();
        l.spec = changed_repo;
        l.spec_hash = lock::spec_hash(&l.spec).unwrap();
        l
    };
    assert!(lock::reconcile(&reordered, Some(&old_multi)).is_err());
}

#[test]
fn old_locks_are_rejected_and_v5_checks_ownership() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    artifact(&repo, "a", "1");
    let spec = spec(&repo, json!({"main":{"dependencies":["t:a:1"]}}));
    let mut legacy = lock::create(&spec).unwrap();
    legacy.version = 1;
    assert!(lock::validate(&legacy, &spec).is_err());
    assert!(lock::reconcile(&spec, Some(&legacy)).is_err());
    let migrated = lock::create(&spec).unwrap();
    assert_eq!(migrated.version, 5);
    assert!(
        migrated.classpath_inputs["main"]
            .models
            .contains("t/a/1/a-1.pom")
    );
    let mut bad = migrated.clone();
    bad.classpath_inputs.clear();
    assert!(lock::validate(&bad, &spec).is_err());
    let mut bad = migrated.clone();
    bad.classpath_inputs
        .get_mut("main")
        .unwrap()
        .provenance
        .remove("t/a/1/a-1.jar");
    assert!(lock::validate(&bad, &spec).is_err());
    let mut bad = migrated;
    bad.classpath_inputs
        .get_mut("main")
        .unwrap()
        .models
        .insert("missing".into());
    assert!(lock::validate(&bad, &spec).is_err());
}

#[test]
fn shared_inputs_conflict_instead_of_overwriting_preserved_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    artifact(&repo, "a", "1");
    let initial = spec(&repo, json!({"main":{"dependencies":["t:a:1"]}}));
    let old = lock::create(&initial).unwrap();
    let changed = spec(
        &repo,
        json!({"main":{"dependencies":["t:a:1"]},"test":{"dependencies":["t:a:1"]}}),
    );
    let shared = lock::reconcile(&changed, Some(&old)).unwrap();
    assert_eq!(shared.artifacts.len(), 1);
    fs::write(repo.join("t/a/1/a-1.jar"), "changed bytes").unwrap();
    let input = temp.path().join("dependencies.json");
    build::write_json(&input, &changed).unwrap();
    let output = temp.path().join("jix.lock");
    lock::write(&old, &output).unwrap();
    let before = fs::read(&output).unwrap();
    let run = Command::new(env!("CARGO_BIN_EXE_jix"))
        .arg("lock")
        .arg(&input)
        .arg("--output")
        .arg(&output)
        .output()
        .unwrap();
    assert!(!run.status.success());
    assert_eq!(fs::read(&output).unwrap(), before);
    assert!(
        lock::reconcile(&changed, Some(&old))
            .unwrap_err()
            .to_string()
            .contains("Conflicting shared")
    );
    artifact(&repo, "a", "1");
    let pom = repo.join("t/a/1/a-1.pom");
    let data = fs::read_to_string(&pom).unwrap();
    fs::write(pom, format!("{data}\n")).unwrap();
    assert!(
        lock::reconcile(&changed, Some(&old))
            .unwrap_err()
            .to_string()
            .contains("Conflicting shared")
    );
}

#[test]
fn info_reads_only_the_lock_and_preserves_order() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    artifact(&repo, "a", "1");
    artifact(&repo, "b", "1");
    let spec = spec(
        &repo,
        json!({"main":{"dependencies":["t:b:1","t:a:1"]},"runtime":{"dependencies":[]}}),
    );
    let old = lock::create(&spec).unwrap();
    let path = temp.path().join("jix.lock");
    lock::write(&old, &path).unwrap();
    fs::remove_dir_all(repo).unwrap();
    let invoke = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_jix"))
            .args(args)
            .current_dir(temp.path())
            .output()
            .unwrap()
    };
    let json = invoke(&["info", "--classpath", "main", "--json"]);
    assert!(json.status.success());
    assert!(json.stderr.is_empty());
    let report: Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(report["lockVersion"], 5);
    assert_eq!(report["classpaths"].as_object().unwrap().len(), 1);
    assert_eq!(
        report["classpaths"]["main"]["artifacts"][0]["id"],
        "t:b:jar::1"
    );
    assert_eq!(
        json.stdout,
        invoke(&["info", "--classpath", "main", "--json"]).stdout
    );
    let human = invoke(&["info"]);
    assert!(human.status.success());
    assert!(String::from_utf8_lossy(&human.stdout).contains("Lock metadata only"));
    assert!(!invoke(&["info", "--classpath", "unknown"]).status.success());
}

#[test]
fn fresh_classpaths_own_only_their_reads_in_a_shared_repository() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    artifact(&repo, "a", "1");
    artifact(&repo, "b", "1");
    let declarations = spec(
        &repo,
        json!({
            "main": {"dependencies": ["t:a:1"]},
            "test": {"dependencies": ["t:b:1", "t:a:1"]}
        }),
    );
    let locked = lock::create(&declarations).unwrap();
    assert_eq!(locked.artifacts.len(), 2);
    assert_eq!(locked.classpath_inputs["main"].provenance.len(), 2);
    assert_eq!(locked.classpath_inputs["test"].provenance.len(), 4);
    let mut removed = declarations;
    removed.classpaths.remove("test");
    fs::remove_dir_all(repo).unwrap();
    let pruned = lock::reconcile(&removed, Some(&locked)).unwrap();
    assert_eq!(pruned.artifacts.len(), 1);
    assert_eq!(pruned.provenance.len(), 2);
}

#[test]
fn declared_context_owns_models_and_invalidates_only_changed_classpath() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    for name in ["a", "b"] {
        artifact(&repo, name, "1");
    }
    let mut initial = spec(
        &repo,
        json!({"main":{"dependencies":["t:a:1"]},"other":{"dependencies":["t:b:1"]}}),
    );
    initial.resolution_context =
        Some(serde_json::from_value(json!({"systemProperties":{"java.version":"21"}})).unwrap());
    let old = lock::create(&initial).unwrap();
    assert_eq!(old.context_models.len(), 1);
    fs::remove_dir_all(repo.join("t/a")).unwrap();
    let mut changed = initial.clone();
    changed
        .classpaths
        .get_mut("other")
        .unwrap()
        .resolution_context =
        Some(serde_json::from_value(json!({"systemProperties":{"java.version":"17"}})).unwrap());
    let new = lock::reconcile(&changed, Some(&old)).unwrap();
    assert_eq!(new.context_models.len(), 2);
    assert_eq!(
        value(&new.classpath_inputs["main"]),
        value(&old.classpath_inputs["main"])
    );
    lock::validate(&new, &changed).unwrap();
    let mut bad = new.clone();
    bad.classpath_inputs.get_mut("other").unwrap().context_hash =
        old.classpath_inputs["main"].context_hash.clone();
    assert!(lock::validate(&bad, &changed).is_err());
    let mut bad = new.clone();
    bad.context_models
        .insert("unowned".into(), Default::default());
    assert!(lock::validate(&bad, &changed).is_err());
}

#[test]
fn old_versions_require_explicit_relocking() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    artifact(&repo, "a", "1");
    let request = spec(&repo, json!({"main":{"dependencies":["t:a:1"]}}));
    let mut old = lock::create(&request).unwrap();
    for version in 1..5 {
        old.version = version;
        assert!(lock::validate(&old, &request).is_err());
        assert!(lock::reconcile(&request, Some(&old)).is_err());
    }
}

#[test]
fn resolution_ignores_host_environment_and_working_directory() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    artifact(&repo, "a", "1");
    let pom = repo.join("t/a/1/a-1.pom");
    fs::write(&pom,fs::read_to_string(&pom).unwrap().replace("</project>","<profiles><profile><id>relative</id><activation><file><exists>marker</exists></file></activation><dependencies><dependency><groupId>missing</groupId><artifactId>not-selected</artifactId><version>1</version></dependency></dependencies></profile><profile><id>env</id><activation><property><name>env.JIX_PROFILE_TEST</name></property></activation><dependencies><dependency><groupId>missing</groupId><artifactId>not-selected</artifactId><version>1</version></dependency></dependencies></profile></profiles></project>")).unwrap();
    let mut request = spec(&repo, json!({"main":{"dependencies":["t:a:1"]}}));
    request.resolution_context = Some(Default::default());
    let input = temp.path().join("dependencies.json");
    build::write_json(&input, &request).unwrap();
    let other = temp.path().join("other");
    fs::create_dir(&other).unwrap();
    fs::write(other.join("marker"), "exists").unwrap();
    let invoke = |cwd: &Path, env: &str| {
        let p = Command::new(env!("CARGO_BIN_EXE_jix"))
            .args(["resolve", input.to_str().unwrap()])
            .current_dir(cwd)
            .env("JIX_PROFILE_TEST", env)
            .env("JAVA_HOME", env)
            .output()
            .unwrap();
        assert!(p.status.success(), "{}", String::from_utf8_lossy(&p.stderr));
        p.stdout
    };
    assert_eq!(invoke(temp.path(), "one"), invoke(&other, "two"));
}
