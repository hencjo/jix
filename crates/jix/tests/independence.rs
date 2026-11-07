#![cfg(not(feature = "nix"))]

use serde_json::{Value, json};
use std::{fs, process::Command};

#[test]
fn explicit_commands_need_neither_nix_nor_host_defaults() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let repository = root.join("repository");
    let artifact = repository.join("t/a/1");
    fs::create_dir_all(&artifact).unwrap();
    fs::write(artifact.join("a-1.pom"), "<project><modelVersion>4.0.0</modelVersion><groupId>t</groupId><artifactId>a</artifactId><version>1</version></project>").unwrap();
    let spec = root.join("dependencies.json");
    fs::write(
        &spec,
        serde_json::to_vec(&json!({
            "repositories":[format!("file://{}", repository.display())],
            "classpaths":{"main":{"scope":"compile","dependencies":["t:a:1"]}}
        }))
        .unwrap(),
    )
    .unwrap();
    let run = |args: &[&str], success: bool| {
        let output = Command::new(env!("CARGO_BIN_EXE_jix"))
            .args(args)
            .current_dir(root)
            .env_clear()
            .env("PATH", root.join("no-tools"))
            .env("HOME", root.join("no-home"))
            .env("JIX_ENVIRONMENT", "not-a-declared-environment")
            .output()
            .unwrap();
        assert_eq!(
            output.status.success(),
            success,
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    };
    // Resolution reads descriptors, not JARs; locking must capture actual bytes.
    let resolved = run(&["resolve", "dependencies.json"], true);
    let graph: Value = serde_json::from_slice(&resolved.stdout).unwrap();
    assert_eq!(
        graph["classpaths"]["main"]["artifacts"][0]["id"],
        "t:a:jar::1"
    );
    run(&["lock", "dependencies.json"], false);
    assert!(!root.join("jix.lock").exists());
    fs::write(artifact.join("a-1.jar"), b"fixture-jar").unwrap();
    run(&["lock", "dependencies.json"], true);
    let locked = fs::read(root.join("jix.lock")).unwrap();
    fs::remove_dir_all(repository).unwrap();
    run(&["lock", "dependencies.json"], true);
    run(
        &["verify", "jix.lock", "--dependencies", "dependencies.json"],
        true,
    );
    run(&["info", "jix.lock", "--json"], true);
    assert_eq!(fs::read(root.join("jix.lock")).unwrap(), locked);

    for args in [vec!["verify"], vec!["build"], vec!["init"]] {
        let failed = run(&args, false);
        assert!(String::from_utf8_lossy(&failed.stderr).contains("without the Nix backend"));
    }
    assert!(!root.join("flake.nix").exists());
}
