use std::{fs, process::Command};

#[test]
fn guide_is_embedded_offline_and_read_only() {
    let directory = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_jix"))
        .args(["migrate", "--guide"])
        .current_dir(directory.path())
        .env_clear()
        .env("PATH", "/nonexistent")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let expected = format!(
        "# Migrating to Jix {}\n\n{}",
        env!("CARGO_PKG_VERSION"),
        include_str!("../docs/migration.md")
    );
    assert_eq!(output.stdout, expected.as_bytes());
    assert!(!output.stdout.contains(&0x1b));
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn migration_invocations_are_explicit() {
    for args in [
        vec!["migrate"],
        vec!["migrate", "--guide", "--guide"],
        vec!["migrate", "--guide", "extra"],
        vec!["migrate", "--guide", "--project", "."],
        vec!["migrate", "--output", "guide.md"],
        vec!["migrate", "--guide", "true"],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_jix"))
            .args(&args)
            .output()
            .unwrap();
        assert!(!result.status.success(), "{args:?}");
        assert!(result.stdout.is_empty());
        assert!(String::from_utf8_lossy(&result.stderr).contains("jix migrate --guide"));
    }
    for flag in ["--help", "-h", "--version", "-V"] {
        let result = Command::new(env!("CARGO_BIN_EXE_jix"))
            .args(["migrate", flag])
            .output()
            .unwrap();
        assert!(result.status.success());
        assert!(result.stderr.is_empty());
        assert!(!String::from_utf8_lossy(&result.stdout).starts_with("# Migrating"));
    }
}
