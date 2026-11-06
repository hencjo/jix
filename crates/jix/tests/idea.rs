use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
};

fn jdk() -> Option<String> {
    let home = std::env::var_os("JAVA_HOME").map(PathBuf::from).into_iter();
    let path = std::env::var_os("PATH").unwrap_or_default();
    let candidates = std::env::split_paths(&path).filter_map(|p| {
        p.join("javac")
            .canonicalize()
            .ok()?
            .parent()?
            .parent()
            .map(Path::to_owned)
    });
    home.chain(candidates)
        .find(|p| p.starts_with("/nix/store") && p.join("bin/javac").is_file())
        .map(|p| p.to_str().unwrap().to_owned())
}
fn model(home: &str) -> String {
    json!({
        "version":1,"sourceRoot":"/snapshot",
        "modules":[{"name":"app","sources":"/snapshot/src","resources":[],"jdk":home,
            "release":21,"javacOptions":[],"compile":{"classpath":[],"modulepath":[]},
            "runtime":{"classpath":[],"modulepath":[]}}],
        "applications":[{"name":"app","mainClass":"example.Main","module":null,"jdk":home,
            "runtime":{"classpath":[{"module":"app"}],"modulepath":[]}}]
    })
    .to_string()
}
#[test]
fn generation_is_owned_idempotent_and_preserves_user_files() {
    let Some(home) = jdk() else {
        eprintln!("Skipping SDK-backed IDEA test: no store JDK");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("src/example")).unwrap();
    fs::write(
        root.join("src/example/Main.java"),
        "package example; public class Main {}",
    )
    .unwrap();
    fs::create_dir(root.join(".idea")).unwrap();
    fs::write(root.join(".idea/workspace.xml"), "user workspace").unwrap();
    jix::idea::generate(root, &model(&home)).unwrap();
    let original = fs::read(root.join(".idea/compiler.xml")).unwrap();
    jix::idea::generate(root, &model(&home)).unwrap();
    assert_eq!(fs::read(root.join(".idea/compiler.xml")).unwrap(), original);
    assert_eq!(
        fs::read_to_string(root.join(".idea/workspace.xml")).unwrap(),
        "user workspace"
    );
    assert!(
        fs::read_to_string(root.join(".idea/jix-modules/app.iml"))
            .unwrap()
            .contains("$MODULE_DIR$/../..")
    );
    fs::write(root.join(".idea/compiler.xml"), "user edit").unwrap();
    assert!(
        jix::idea::generate(root, &model(&home))
            .unwrap_err()
            .to_string()
            .contains("Modified")
    );
    assert_eq!(
        fs::read_to_string(root.join(".idea/compiler.xml")).unwrap(),
        "user edit"
    );
    fs::write(root.join(".idea/compiler.xml"), &original).unwrap();
    fs::remove_file(root.join(".idea/compiler.xml")).unwrap();
    assert!(
        jix::idea::generate(root, &model(&home))
            .unwrap_err()
            .to_string()
            .contains("removed")
    );
    fs::write(root.join(".idea/compiler.xml"), &original).unwrap();
    fs::write(root.join(".jix/idea-write.lock"), "").unwrap();
    assert!(
        jix::idea::generate(root, &model(&home))
            .unwrap_err()
            .to_string()
            .contains("writer")
    );
}
#[test]
fn unmanaged_and_unsafe_destinations_fail_before_publication() {
    let Some(home) = jdk() else {
        eprintln!("Skipping SDK-backed IDEA test: no store JDK");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("src/example")).unwrap();
    fs::write(
        root.join("src/example/Main.java"),
        "package example; public class Main {}",
    )
    .unwrap();
    fs::create_dir(root.join(".idea")).unwrap();
    fs::write(root.join(".idea/modules.xml"), "existing IDE project").unwrap();
    assert!(
        jix::idea::generate(root, &model(&home))
            .unwrap_err()
            .to_string()
            .contains("Unmanaged")
    );
    assert!(!root.join(".idea/compiler.xml").exists());
    assert!(!root.join(".jix/idea-state.json").exists());
    fs::remove_file(root.join(".idea/modules.xml")).unwrap();
    std::os::unix::fs::symlink("/tmp", root.join(".idea/jix-modules")).unwrap();
    assert!(jix::idea::generate(root, &model(&home)).is_err());
    assert!(!root.join(".idea/compiler.xml").exists());
    fs::remove_file(root.join(".idea/jix-modules")).unwrap();
    fs::write(
        root.join(".jix/idea-state.json"),
        json!({"version":1,"files":{".idea/workspace.xml":"forged"}}).to_string(),
    )
    .unwrap();
    assert!(jix::idea::generate(root, &model(&home)).is_err());
    assert!(!root.join(".idea/compiler.xml").exists());
}

#[test]
fn v2_multiple_local_roots_and_overlap_validation() {
    let Some(home) = jdk() else {
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("second/example")).unwrap();
    fs::write(
        root.join("second/example/Main.java"),
        "package example; public class Main {}",
    )
    .unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&model(&home)).unwrap();
    value["version"] = json!(2);
    value["modules"][0]["sources"] = json!([
        {"path":"/snapshot/src","generated":false},
        {"path":"/snapshot/second","generated":false}
    ]);
    jix::idea::generate(root, &value.to_string()).unwrap();
    let iml = fs::read_to_string(root.join(".idea/jix-modules/app.iml")).unwrap();
    assert!(iml.contains("/src") && iml.contains("/second"));
    value["modules"][0]["sources"][1]["path"] = json!("/snapshot/src");
    assert!(jix::idea::generate(root, &value.to_string()).is_err());
    value["modules"][0]["sources"] = json!([{"path":"/tmp/not-a-store-root","generated":true}]);
    assert!(jix::idea::generate(root, &value.to_string()).is_err());
}
