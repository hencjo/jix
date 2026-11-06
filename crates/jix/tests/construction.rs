use jix::{artifacts, build, lock};
use serde_json::{Value, json};
use std::{fs, path::Path};
fn execute(root: &Path, mode: &str, config: Value, name: &str) -> anyhow::Result<()> {
    build::write_json(&root.join("config.json"), &config)?;
    build::run(mode, &root.join("config.json"), &root.join(name))
}
#[test]
fn reproducible_archive_and_safety() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir(root.join("classes")).unwrap();
    fs::write(root.join("classes/Main.class"), b"class").unwrap();
    fs::write(root.join("message.txt"), b"hello").unwrap();
    let config = json!({"classes":"classes","resources":[{"from":"message.txt","into":"data/message.txt"}],"manifest":{"Main-Class":"Main"}});
    execute(root, "jar", config.clone(), "a.jar").unwrap();
    execute(root, "jar", config.clone(), "b.jar").unwrap();
    assert_eq!(
        fs::read(root.join("a.jar")).unwrap(),
        fs::read(root.join("b.jar")).unwrap()
    );
    let mut jar = zip::ZipArchive::new(fs::File::open(root.join("a.jar")).unwrap()).unwrap();
    assert_eq!(jar.by_index(0).unwrap().name(), "META-INF/MANIFEST.MF");
    assert_eq!(jar.len(), 3);
    for i in 0..jar.len() {
        assert_eq!(
            jar.by_index(i).unwrap().last_modified().unwrap(),
            zip::DateTime::default()
        );
    }
    for into in [
        "../escape",
        "/absolute",
        "a/../b",
        "a\\b",
        "a//b",
        "Main.class",
        "META-INF/MANIFEST.MF",
        "META-INF",
        "Main.class/child",
    ] {
        let mut bad = config.clone();
        bad["resources"][0]["into"] = json!(into);
        assert!(execute(root, "jar", bad, "bad.jar").is_err(), "{into}");
        assert!(!root.join("bad.jar").exists());
    }
    std::os::unix::fs::symlink(root.join("message.txt"), root.join("link")).unwrap();
    for source in ["link", "missing"] {
        let mut bad = config.clone();
        bad["resources"][0]["from"] = json!(source);
        assert!(execute(root, "jar", bad, "bad.jar").is_err());
    }
    assert!(execute(root, "jar", config, "a.jar").is_err());
}
#[test]
fn manifest_folding() {
    let value = "å".repeat(100);
    let mut attributes = std::collections::BTreeMap::new();
    attributes.insert("Description".into(), value.clone());
    let data = String::from_utf8(build::manifest(&attributes).unwrap()).unwrap();
    assert!(data.split("\r\n").all(|l| l.len() <= 72));
    assert!(data.replace("\r\n ", "").contains(&value));
    for bad in [
        json!({"Main-Class":"x\ninjected"}),
        json!({"Main-Class":"x","main-class":"y"}),
        json!({"Class-Path":"/nix/store/abc"}),
        json!({"Manifest-Version":"2.0"}),
        json!({"invalid key":"x"}),
    ] {
        assert!(build::manifest(&serde_json::from_value(bad).unwrap()).is_err());
    }
}
#[test]
fn classpath_order_conflict_and_compile_options() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::write(root.join("a.jar"), b"a").unwrap();
    fs::write(root.join("b.jar"), b"b").unwrap();
    let a = json!({"id":"t:a:jar::1","key":"t:a:jar:","version":"1","path":"a.jar"});
    let b = json!({"id":"t:b:jar::1","key":"t:b:jar:","version":"1","path":"b.jar"});
    execute(root, "classpath", json!({"entries":[a,b,a]}), "cp").unwrap();
    let values: Vec<build::Entry> = build::read_json(&root.join("cp/entries.json")).unwrap();
    assert_eq!(values.len(), 2);
    assert!(values[0].path.ends_with("a.jar"));
    let mut changed = a.clone();
    changed["version"] = json!("2");
    changed["id"] = json!("t:a:jar::2");
    assert!(execute(root, "classpath", json!({"entries":[a,changed]}), "bad").is_err());
    assert!(
        execute(
            root,
            "compile",
            json!({"sources":"src","javac":"javac","release":21,"javacOptions":["-processor"]}),
            "classes"
        )
        .is_err()
    );
}
#[test]
fn export_verify_and_relocation() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let repo = root.join("repo");
    let dir = repo.join("t/a/1");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("a-1.pom"),"<project><modelVersion>4.0.0</modelVersion><groupId>t</groupId><artifactId>a</artifactId><version>1</version></project>").unwrap();
    fs::write(dir.join("a-1.jar"), b"jar").unwrap();
    let spec=serde_json::from_value(json!({"repositories":[format!("file://{}",repo.display())],"classpaths":{"main":{"dependencies":["t:a:1"]}}})).unwrap();
    let locked = lock::create(&spec).unwrap();
    let wire = serde_json::to_value(&locked).unwrap();
    assert!(wire.get("request").is_some());
    assert!(wire.get("requestHash").is_some());
    assert!(wire.get("spec").is_none());
    let input = root.join("input");
    fs::create_dir(&input).unwrap();
    lock::write(&locked, &input.join("jix.lock")).unwrap();
    let map = artifacts::Manifest {
        selection: None,
        version: 1,
        artifacts: locked
            .provenance
            .keys()
            .map(|p| (p.clone(), repo.join(p)))
            .collect(),
    };
    build::write_json(&input.join("artifacts.json"), &map).unwrap();
    artifacts::verify(&locked, &input.join("artifacts.json"), None).unwrap();
    artifacts::materialize(&input, &root.join("export")).unwrap();
    assert!(artifacts::materialize(&input, &root.join("export")).is_err());
    fs::rename(root.join("export"), root.join("moved")).unwrap();
    fs::remove_dir_all(&repo).unwrap();
    artifacts::verify(&locked, &root.join("moved/artifacts.json"), None).unwrap();
    build::run(
        "classpath",
        &root.join("moved/classpaths/main/classpath.json"),
        &root.join("cp"),
    )
    .unwrap();
    fs::write(root.join("moved/unrelated"), b"extra").unwrap();
    artifacts::verify(&locked, &root.join("moved/artifacts.json"), None).unwrap();
    fs::write(root.join("moved/repository/t/a/1/a-1.pom"), b"bad").unwrap();
    assert!(artifacts::verify(&locked, &root.join("moved/artifacts.json"), None).is_err());
    artifacts::verify(&locked, &root.join("moved/artifacts.json"), Some("main")).unwrap();
    fs::remove_file(root.join("moved/repository/t/a/1/a-1.jar")).unwrap();
    assert!(artifacts::verify(&locked, &root.join("moved/artifacts.json"), Some("main")).is_err());
    assert!(
        artifacts::verify(&locked, &root.join("moved/artifacts.json"), Some("unknown")).is_err()
    );
}

#[test]
fn source_library_and_modulepath_identity() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::write(root.join("hash-library.jar"), b"jar").unwrap();
    let library = json!({"kind":"library","id":"example.core","key":"example.core",
        "version":"","jarName":"core.jar","path":"hash-library.jar"});
    execute(
        root,
        "modulepath",
        json!({"entries":[library,library]}),
        "mp",
    )
    .unwrap();
    assert!(root.join("mp/jars/core.jar").is_file());
    assert_eq!(fs::read_dir(root.join("mp/jars")).unwrap().count(), 1);
    let mut other = library.clone();
    other["id"] = json!("other");
    other["key"] = json!("other");
    assert!(
        execute(
            root,
            "modulepath",
            json!({"entries":[library,other]}),
            "collision"
        )
        .is_err()
    );
    for name in ["../core.jar", "core", "/core.jar", ".jar", "bad name.jar"] {
        let mut bad = library.clone();
        bad["jarName"] = json!(name);
        assert!(execute(root, "modulepath", json!({"entries":[bad]}), "bad").is_err());
    }
    let maven = json!({"id":"t:a:jar::1","key":"t:a:jar:","version":"1","path":"hash-library.jar"});
    execute(root, "modulepath", json!({"entries":[maven]}), "maven").unwrap();
    assert!(root.join("maven/jars/a-1.jar").is_file());
    fs::create_dir(root.join("src")).unwrap();
    fs::write(root.join("src/module-info.java"), "module example.core {}").unwrap();
    let config = json!({"sources":"src","javac":"/not-invoked","release":8});
    assert!(
        execute(root, "compile", config, "old-release")
            .unwrap_err()
            .to_string()
            .contains("release >= 9")
    );
    let config = json!({"sources":"src","javac":"/not-invoked","release":21,
        "classpaths":["mp/entries.json"],"modulepaths":["mp/entries.json"]});
    assert!(
        execute(root, "compile", config, "overlap")
            .unwrap_err()
            .to_string()
            .contains("both classpath")
    );
}

#[test]
fn named_module_uses_declared_source_root() {
    let javac = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|dir| dir.join("javac"))
        .find(|path| path.is_file());
    let Some(javac) = javac else {
        eprintln!("JDK unavailable; the Nix gate runs this test with the pinned JDK");
        return;
    };
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("src/core")).unwrap();
    fs::write(
        root.join("src/module-info.java"),
        "module example.core { exports core; }",
    )
    .unwrap();
    fs::write(
        root.join("src/core/Core.java"),
        "package core; public class Core {}",
    )
    .unwrap();
    execute(
        root,
        "compile",
        json!({"sources":"src","javac":javac,"release":21}),
        "classes",
    )
    .unwrap();
    assert!(root.join("classes/module-info.class").is_file());
    execute(root, "jar", json!({"classes":"classes"}), "core.jar").unwrap();
    execute(
        root,
        "modulepath",
        json!({"entries":[{
            "kind":"library","id":"core","key":"core","version":"",
            "jarName":"core.jar","path":"core.jar"
        }]}),
        "core-path",
    )
    .unwrap();
    fs::create_dir_all(root.join("consumer/user")).unwrap();
    fs::write(
        root.join("consumer/module-info.java"),
        "module example.user { requires example.core; }",
    )
    .unwrap();
    fs::write(
        root.join("consumer/user/User.java"),
        "package user; public class User { core.Core value; }",
    )
    .unwrap();
    let config = json!({"sources":"consumer","javac":javac,"release":21,"modulepaths":["core-path/entries.json"]});
    execute(root, "compile", config.clone(), "user-classes").unwrap();
    fs::write(
        root.join("consumer/module-info.java"),
        "module example.user {}",
    )
    .unwrap();
    assert!(execute(root, "compile", config, "unreadable").is_err());
    fs::remove_file(root.join("consumer/module-info.java")).unwrap();
    execute(
        root,
        "compile",
        json!({"sources":"consumer","javac":javac,"release":21,
        "modulepaths":["core-path/entries.json"]}),
        "unnamed-classes",
    )
    .unwrap();
}

#[test]
fn source_roots_are_checked_before_compilation() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    for dir in ["one/nested", "two", "empty"] {
        fs::create_dir_all(root.join(dir)).unwrap();
    }
    let config = |sources: Value| json!({"sources":sources,"javac":"/not-invoked","release":21});
    for sources in [
        json!(["one", "one"]),
        json!(["one", "one/nested"]),
        json!(["missing"]),
    ] {
        assert!(execute(root, "compile", config(sources), "bad").is_err());
        assert!(!root.join("bad").exists());
    }
    fs::write(root.join("file"), "").unwrap();
    assert!(execute(root, "compile", config(json!(["file"])), "bad").is_err());
    std::os::unix::fs::symlink(root.join("one"), root.join("linked")).unwrap();
    assert!(execute(root, "compile", config(json!(["linked"])), "bad").is_err());
    fs::write(root.join("one/A.java"), "class A {}").unwrap();
    fs::write(root.join("two/A.java"), "class A {}").unwrap();
    let err = execute(root, "compile", config(json!(["one", "two"])), "bad")
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("Duplicate Java source") && err.contains("one") && err.contains("two"),
        "{err}"
    );
    fs::remove_file(root.join("two/A.java")).unwrap();
    std::os::unix::fs::symlink(root.join("one/A.java"), root.join("two/B.java")).unwrap();
    assert!(execute(root, "compile", config(json!(["two"])), "bad").is_err());
    fs::remove_file(root.join("two/B.java")).unwrap();
    fs::write(root.join("one/module-info.java"), "module one {}").unwrap();
    fs::write(root.join("two/module-info.java"), "module two {}").unwrap();
    assert!(execute(root, "compile", config(json!(["one", "two"])), "bad").is_err());
    execute(root, "compile", config(json!(["empty"])), "empty-classes").unwrap();
    execute(root, "compile", config(json!([])), "no-classes").unwrap();
}

#[test]
fn compile_multiple_roots_with_and_without_jpms() {
    let Some(javac) = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|dir| dir.join("javac"))
        .find(|path| path.is_file())
    else {
        eprintln!("Skipping multi-root compilation: JDK unavailable");
        return;
    };
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("hand/example")).unwrap();
    fs::create_dir_all(root.join("generated/example")).unwrap();
    fs::write(
        root.join("hand/example/Main.java"),
        "package example; public class Main { Generated value; }",
    )
    .unwrap();
    fs::write(
        root.join("generated/example/Generated.java"),
        "package example; public class Generated { Main value; }",
    )
    .unwrap();
    let config = json!({"sources":["hand","generated"],"javac":javac,"release":21});
    execute(root, "compile", config.clone(), "plain").unwrap();
    fs::write(
        root.join("hand/module-info.java"),
        "module example { exports example; }",
    )
    .unwrap();
    execute(root, "compile", config, "modular").unwrap();
    for output in ["plain", "modular"] {
        assert!(root.join(output).join("example/Main.class").is_file());
        assert!(root.join(output).join("example/Generated.class").is_file());
    }
    assert!(root.join("modular/module-info.class").is_file());
}
