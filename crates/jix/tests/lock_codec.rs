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
