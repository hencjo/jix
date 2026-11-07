use jix::{lock, lock_codec, matrix};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

fn fixture() -> (tempfile::TempDir, Value) {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("missing")).unwrap();
    let dir = temp.path().join("t/a/1");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("a-1.pom"), "<project><modelVersion>4.0.0</modelVersion><groupId>t</groupId><artifactId>a</artifactId><version>1</version></project>").unwrap();
    fs::write(dir.join("a-1.jar"), b"jar").unwrap();
    let spec = json!({"repositories":[format!("file://{}/missing",temp.path().display()), format!("file://{}",temp.path().display())],
        "classpaths":{"main":{"dependencies":["t:a:1"]},
        "runtime":{"scope":"runtime","dependencies":["t:a:1"],"resolutionContext":{}}}});
    (temp, spec)
}

// The same mutated documents exercise Rust verification and both Nix entrypoints.
// No case needs to contact (or even have) the repository named in the lock.
fn validation_cases() -> Vec<(String, Value, bool)> {
    let (_temp, spec) = fixture();
    let single = lock::create(&serde_json::from_value(spec.clone()).unwrap()).unwrap();
    let matrix_spec = matrix::normalize(json!({"repositories":spec["repositories"],
        "classpaths":{"main":{"dependencies":["t:a:1"]}},
        "environments":{"a":{"resolutionContext":{}},"b":{"resolutionContext":{}}}}))
    .unwrap();
    let matrix = matrix::reconcile(&matrix_spec, None).unwrap();
    let mut cases = Vec::new();
    for (kind, original) in [
        (
            "single",
            lock_codec::encode(serde_json::to_value(single).unwrap()).unwrap(),
        ),
        (
            "matrix",
            lock_codec::encode(serde_json::to_value(matrix).unwrap()).unwrap(),
        ),
    ] {
        cases.push((format!("{kind}-original"), original.clone(), true));
        let mut duplicate = original.clone();
        // In a matrix, corrupt b while the Nix test also consumes a's classpath.
        let group = if kind == "matrix" {
            &mut duplicate["resolution"]["environments"]["b"]
        } else {
            &mut duplicate["resolution"]
        };
        let artifacts = group["classpaths"]["main"]["artifacts"]
            .as_array_mut()
            .unwrap();
        artifacts.push(artifacts[0].clone());
        cases.push((format!("{kind}-duplicate"), duplicate, false));

        let mut conflicting = original.clone();
        let group = if kind == "matrix" {
            &mut conflicting["resolution"]["environments"]["b"]
        } else {
            &mut conflicting["resolution"]
        };
        let mut other = group["classpaths"]["main"]["artifacts"][0].clone();
        other["id"] = json!("t:a:jar::2");
        group["classpaths"]["main"]["artifacts"]
            .as_array_mut()
            .unwrap()
            .push(other);
        conflicting["provenance"]["t/a/2/a-2.jar"] =
            conflicting["provenance"]["t/a/1/a-1.jar"].clone();
        let inputs = if kind == "matrix" {
            &mut conflicting["classpathInputs"]["b"]
        } else {
            &mut conflicting["classpathInputs"]
        };
        inputs["main"]["provenance"]
            .as_array_mut()
            .unwrap()
            .push(json!("t/a/2/a-2.jar"));
        cases.push((format!("{kind}-conflicting-version"), conflicting, false));

        for (index, (repository, accepted)) in [
            ("https://example.invalid/maven", true),
            ("https://example.invalid:8443/maven/", true),
            ("http://localhost:8080/maven", true),
            ("http://127.0.0.1/maven", true),
            ("file:///nonexistent/repository", true),
            // Preserve Rust's exact byte policy, including non-whitespace controls.
            ("file:///nonexistent/\u{7f}", true),
            ("file:///nonexistent/\u{b}", true),
            ("http://example.invalid/maven", false),
            ("http://localhost.example.invalid/maven", false),
            ("ftp://example.invalid/maven", false),
            ("https:///maven", false),
            ("https://user:password@example.invalid/maven", false),
            ("https://example.invalid/../secret", false),
            ("https://example.invalid/./maven", false),
            ("https://example.invalid/maven?token=secret", false),
            ("https://example.invalid/maven#fragment", false),
            ("https://example.invalid/maven%20path", false),
            ("https://example.invalid/maven\\path", false),
            ("https://example.invalid/é", false),
            ("https://example.invalid/maven path", false),
            ("https://example.invalid/maven\tpath", false),
            ("https://example.invalid/maven\npath", false),
            ("https://example.invalid/maven\rpath", false),
            ("https://example.invalid/maven\u{c}path", false),
            ("file://relative", false),
            ("file:///nonexistent/../repository", false),
        ]
        .into_iter()
        .enumerate()
        {
            let mut document = original.clone();
            document["request"]["repositories"] = json!([repository]);
            document["requestHash"] = json!(if kind == "matrix" {
                serde_json::from_value::<matrix::Spec>(document["request"].clone())
                    .unwrap()
                    .hash()
                    .unwrap()
            } else {
                lock::spec_hash(&serde_json::from_value(document["request"].clone()).unwrap())
                    .unwrap()
            });
            for source in document["provenance"].as_object_mut().unwrap().values_mut() {
                source["repository"] = json!(0);
            }
            cases.push((format!("{kind}-repository-{index}"), document, accepted));
        }
    }
    cases
}

#[test]
fn lock_validation_fixtures_match_rust_verification() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("jix.lock");
    for (name, disk, accepted) in validation_cases() {
        fs::write(&input, serde_json::to_vec(&disk).unwrap()).unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_jix"))
            .arg("verify")
            .arg(&input)
            .output()
            .unwrap();
        assert_eq!(
            result.status.success(),
            accepted,
            "{name}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}

#[test]
#[ignore = "requires Nix daemon/store access"]
fn rust_nix_lock_validation_parity() {
    let temp = tempfile::tempdir().unwrap();
    let mut cases = Vec::new();
    for (name, disk, accepted) in validation_cases() {
        assert_eq!(
            lock_codec::decode(disk.clone()).is_ok(),
            accepted,
            "{name}: Rust"
        );
        let file = temp.path().join(format!("{name}.json"));
        fs::write(&file, serde_json::to_vec(&disk).unwrap()).unwrap();
        cases.push(json!({"name":name,"file":file,"accepted":accepted}));
    }
    let manifest = temp.path().join("cases.json");
    fs::write(&manifest, serde_json::to_vec(&cases).unwrap()).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let expression = format!(
        r#"let
          flake = builtins.getFlake {};
          pkgs = import flake.inputs.nixpkgs {{ system = builtins.currentSystem; }};
          api = import {}/nix/dependencies.nix {{ inherit pkgs; jix = "/unused-jix"; }};
          cases = builtins.fromJSON (builtins.readFile {});
        in map (c: let
          disk = builtins.fromJSON (builtins.readFile c.file);
          deps = api.dependencies {{ spec = disk.request; lockFile = c.file; }};
          selected = if disk.kind == "matrix" then deps.environments else {{ single = deps; }};
        in {{
          inherit (c) name;
          lockValid = (builtins.tryEval deps.descriptor.lockValid).success;
          classpaths = (builtins.tryEval (builtins.deepSeq
            (map (env: env.classpaths.main.drvPath) (builtins.attrValues selected)) true)).success;
        }}) cases"#,
        json!(format!("path:{}", root.display())),
        root.display(),
        json!(manifest),
    );
    let result = Command::new("nix")
        .args([
            "eval",
            "--offline",
            "--impure",
            "--json",
            "--expr",
            &expression,
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let results: Vec<Value> = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(results.len(), cases.len());
    for (actual, case) in results.iter().zip(&cases) {
        assert_eq!(actual["name"], case["name"]);
        for gate in ["lockValid", "classpaths"] {
            assert_eq!(actual[gate], case["accepted"], "{}: {gate}", case["name"]);
        }
    }
}

#[test]
#[ignore = "requires Nix daemon/store access"]
fn rust_nix_compact_codec_parity() {
    let (temp, single_spec) = fixture();
    let single = lock::create(&serde_json::from_value(single_spec.clone()).unwrap()).unwrap();
    let matrix_spec = matrix::normalize(json!({"repositories":single_spec["repositories"],
        "classpaths":{"main":{"dependencies":["t:a:1"]}},
        "environments":{"a":{"resolutionContext":{}},"b":{"resolutionContext":{"systemProperties":{"os.arch":"aarch64"}}}}})).unwrap();
    let matrix = matrix::reconcile(&matrix_spec, None).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    for mut canonical in [
        serde_json::to_value(single).unwrap(),
        serde_json::to_value(matrix).unwrap(),
    ] {
        // Unused management values are raw metadata, not selected coordinates.
        for models in canonical["contextModels"]
            .as_object_mut()
            .unwrap()
            .values_mut()
        {
            for model in models.as_object_mut().unwrap().values_mut() {
                model["management"] = json!([{"group":"raw:group","artifact":"a%name",
                    "version":"[1:2,)","type":"","classifier":"","scope":"",
                    "optional":false,"optionalValue":false,"exclusions":[]}]);
            }
        }
        let paths: serde_json::Map<String, Value> = canonical["artifacts"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(id, a)| (id.clone(), json!({"path":a["path"]})))
            .collect();
        let compact = lock_codec::encode(canonical.clone()).unwrap();
        let input = temp.path().join("lock.json");
        fs::write(&input, serde_json::to_vec(&compact).unwrap()).unwrap();
        let prefix = format!(
            r#"let
          flake = builtins.getFlake {};
          paths = builtins.fromJSON {};
          codec = import {}/nix/lock-codec.nix {{
            lib = flake.inputs.nixpkgs.lib; artifactParts = id: paths.${{id}};
          }};
          disk = builtins.fromJSON (builtins.readFile {});
          in "#,
            json!(format!("path:{}", root.display())),
            json!(Value::Object(paths).to_string()),
            root.display(),
            input.display()
        );
        for (expr, expected) in [
            ("codec.decode disk", &canonical),
            ("codec.encode (codec.decode disk)", &compact),
        ] {
            let result = Command::new("nix")
                .args([
                    "eval",
                    "--impure",
                    "--json",
                    "--expr",
                    &format!("{prefix}{expr}"),
                ])
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(
                serde_json::from_slice::<Value>(&result.stdout).unwrap(),
                *expected
            );
        }
    }
}

#[test]
fn old_locks_fail_with_upgrade_guidance_and_are_not_replaced() {
    let (temp, spec) = fixture();
    let canonical =
        serde_json::to_value(lock::create(&serde_json::from_value(spec.clone()).unwrap()).unwrap())
            .unwrap();
    let input = temp.path().join("dependencies.json");
    let output = temp.path().join("jix.lock");
    fs::write(&input, serde_json::to_vec(&spec).unwrap()).unwrap();
    fs::remove_dir_all(temp.path().join("t")).unwrap();
    for version in 1..5 {
        let mut old = canonical.clone();
        old["version"] = json!(version);
        let bytes = serde_json::to_vec(&old).unwrap();
        fs::write(&output, &bytes).unwrap();
        for command in ["lock", "verify"] {
            let mut cmd = Command::new(env!("CARGO_BIN_EXE_jix"));
            cmd.arg(command);
            if command == "lock" {
                cmd.arg(&input).arg("--output");
            }
            let result = cmd.arg(&output).output().unwrap();
            assert!(!result.status.success());
            assert!(String::from_utf8_lossy(&result.stderr).contains("move jix.lock aside"));
            assert_eq!(fs::read(&output).unwrap(), bytes);
        }
    }
}
