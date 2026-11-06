use jix::build;
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
};

fn input(root: &Path, name: &str, files: &[(&str, &[u8])]) -> Value {
    let mut zip = zip::ZipWriter::new(fs::File::create(root.join(name)).unwrap());
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (path, bytes) in files {
        zip.start_file(*path, options).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
    json!({"kind":"library","id":name,"key":name,"version":"","jarName":name,"path":name})
}
fn assemble(root: &Path, entries: Value, output: &str) -> anyhow::Result<()> {
    let config = json!({"entries":entries,"mainClass":"example.Main"});
    build::write_json(&root.join("config.json"), &config)?;
    build::run("fat-jar", &root.join("config.json"), &root.join(output))
}
fn contents(root: &Path, archive: &str, name: &str) -> String {
    let mut jar = zip::ZipArchive::new(fs::File::open(root.join(archive)).unwrap()).unwrap();
    let mut text = String::new();
    jar.by_name(name)
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    text
}
#[test]
fn deterministic_services_licenses_and_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let a = input(
        root,
        "a.jar",
        &[
            (
                "META-INF/MANIFEST.MF",
                b"Manifest-Version: 1.0\r\nMulti-Release: true\r\n\r\n",
            ),
            (
                "META-INF/services/example.Service",
                b"example.A # comment\nexample.B\n",
            ),
            ("LICENSE", b"license A"),
            ("META-INF/SIGN.SF", b"signature"),
            ("module-info.class", b"descriptor"),
            ("META-INF/versions/21/example/Main.class", b"modern"),
            ("example/Main.class", b"class"),
            ("same", b"identical"),
        ],
    );
    let b = input(
        root,
        "b.jar",
        &[
            (
                "META-INF/services/example.Service",
                b"example.B\nexample.C\n",
            ),
            ("LICENSE", b"license B"),
            ("same", b"identical"),
        ],
    );
    assemble(root, json!([a, b]), "one.jar").unwrap();
    assemble(root, json!([a, b]), "two.jar").unwrap();
    assert_eq!(
        fs::read(root.join("one.jar")).unwrap(),
        fs::read(root.join("two.jar")).unwrap()
    );
    assert_eq!(
        contents(root, "one.jar", "META-INF/services/example.Service"),
        "example.A\nexample.B\nexample.C\n"
    );
    assert_eq!(
        contents(root, "one.jar", "META-INF/jix/licenses/00000/LICENSE"),
        "license A"
    );
    assert_eq!(
        contents(root, "one.jar", "META-INF/jix/licenses/00001/LICENSE"),
        "license B"
    );
    assert!(contents(root, "one.jar", "META-INF/MANIFEST.MF").contains("Multi-Release: true"));
    let mut zip = zip::ZipArchive::new(fs::File::open(root.join("one.jar")).unwrap()).unwrap();
    assert!(zip.by_name("module-info.class").is_err());
    assert!(zip.by_name("META-INF/SIGN.SF").is_err());
}
#[test]
fn rejects_conflicts_traversal_and_unmarked_multi_release_atomically() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let a = input(root, "a.jar", &[("example/Main.class", b"one")]);
    let b = input(root, "b.jar", &[("example/Main.class", b"two")]);
    assert!(assemble(root, json!([a, b]), "bad.jar").is_err());
    assert!(!root.join("bad.jar").exists());
    for path in [
        "../escape",
        "/absolute",
        "a/../b",
        "a\\b",
        "META-INF/versions/21/X.class",
        "META-INF/jix/origins.json",
    ] {
        let bad = input(root, "bad-input.jar", &[(path, b"bad")]);
        assert!(assemble(root, json!([bad]), "bad.jar").is_err(), "{path}");
        assert!(!root.join("bad.jar").exists());
    }
    let bad = input(root, "bad-input.jar", &[("a", b"bad"), ("a/b", b"bad")]);
    assert!(assemble(root, json!([bad]), "bad.jar").is_err());
    assemble(root, json!([a]), "ok.jar").unwrap();
    let original = fs::read(root.join("ok.jar")).unwrap();
    assert!(assemble(root, json!([a]), "ok.jar").is_err());
    assert_eq!(fs::read(root.join("ok.jar")).unwrap(), original);
}

#[test]
fn rejects_zip_symlinks_bombs_and_failed_helpers() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let entry = input(root, "input.jar", &[]);
    {
        let mut zip = zip::ZipWriter::new(fs::File::create(root.join("input.jar")).unwrap());
        zip.add_symlink(
            "link",
            "../outside",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.finish().unwrap();
    }
    assert!(
        assemble(root, json!([entry]), "bad.jar")
            .unwrap_err()
            .to_string()
            .contains("symlink")
    );
    {
        let mut zip = zip::ZipWriter::new(fs::File::create(root.join("input.jar")).unwrap());
        zip.start_file(
            "bomb",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
        std::io::copy(&mut std::io::repeat(0).take(64 * 1024 * 1024 + 1), &mut zip).unwrap();
        zip.finish().unwrap();
    }
    assert!(
        assemble(root, json!([entry]), "bad.jar")
            .unwrap_err()
            .to_string()
            .contains("limits")
    );
    assert!(!root.join("bad.jar").exists());
    let entry = input(root, "input.jar", &[("example/Main.class", b"class")]);
    build::write_json(
        &root.join("config.json"),
        &json!({
            "entries":[entry], "mainClass":"example.Main",
            "relocations":[{"from":"example","to":"internal.example"}],
            "relocator":root.join("missing-helper")
        }),
    )
    .unwrap();
    assert!(build::run("fat-jar", &root.join("config.json"), &root.join("bad.jar")).is_err());
    assert!(!root.join("bad.jar").exists());
}
