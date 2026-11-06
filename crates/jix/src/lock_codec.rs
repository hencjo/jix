//! Versioned on-disk representation, separate from resolver/canonical serialization.
use anyhow::{Context, Result, ensure};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

pub const VERSION: u32 = 5;
pub const UPGRADE: &str = "Unsupported lock format; upgrade Jix and its flake input, move jix.lock aside, then run jix lock (repository access required)";

fn fields(value: &Value, allowed: &[&str]) -> Result<()> {
    let object = value.as_object().context("Expected lock object")?;
    for key in object.keys() {
        ensure!(allowed.contains(&key.as_str()), "Unknown lock field: {key}");
    }
    Ok(())
}

// Model metadata can contain unused, unvalidated values. Escape delimiters rather
// than treating compact model records as user-selected Maven coordinates.
fn escape_component(value: &str) -> String {
    value.replace('%', "%25").replace(':', "%3A")
}
fn unescape_component(value: &str) -> Result<String> {
    let decoded = value.replace("%3A", ":").replace("%25", "%");
    ensure!(
        escape_component(&decoded) == value,
        "Invalid module component escape"
    );
    Ok(decoded)
}

fn dependency(value: &mut Value, expand: bool) -> Result<()> {
    if expand {
        fields(
            value,
            &[
                "module",
                "type",
                "classifier",
                "scope",
                "optional",
                "optionalValue",
                "exclusions",
            ],
        )?;
        let module = value["module"]
            .as_str()
            .context("Missing dependency module")?;
        let parts: Vec<String> = module
            .split(':')
            .map(unescape_component)
            .collect::<Result<_>>()?;
        ensure!(
            parts.len() == 3,
            "Expected group:artifact:version in locked model"
        );
        let obj = value.as_object_mut().unwrap();
        obj.remove("module");
        for (key, part) in ["group", "artifact", "version"].into_iter().zip(parts) {
            obj.insert(key.into(), json!(part));
        }
    } else {
        let module = format!(
            "{}:{}:{}",
            escape_component(value["group"].as_str().context("group")?),
            escape_component(value["artifact"].as_str().context("artifact")?),
            escape_component(value["version"].as_str().context("version")?)
        );
        let obj = value.as_object_mut().context("dependency")?;
        for key in ["group", "artifact", "version"] {
            obj.remove(key);
        }
        obj.insert("module".into(), json!(module));
    }
    let obj = value.as_object_mut().context("dependency")?;
    for (key, default) in [
        ("type", json!("jar")),
        ("classifier", json!("")),
        ("scope", json!("")),
        ("optional", json!(false)),
        ("exclusions", json!([])),
    ] {
        if expand {
            obj.entry(key).or_insert(default);
        } else if obj.get(key) == Some(&default) {
            obj.remove(key);
        }
    }
    Ok(())
}

fn models_and_resolution(value: &mut Value, expand: bool) -> Result<()> {
    if let Some(contexts) = value.get_mut("contextModels") {
        for models in contexts
            .as_object_mut()
            .context("contextModels")?
            .values_mut()
        {
            for model in models.as_object_mut().context("models")?.values_mut() {
                fields(model, &["dependencies", "management", "relocation"])?;
                for key in ["dependencies", "management"] {
                    if expand {
                        model
                            .as_object_mut()
                            .unwrap()
                            .entry(key)
                            .or_insert(json!([]));
                    }
                    if let Some(list) = model.get_mut(key) {
                        for dep in list.as_array_mut().context("model dependencies")? {
                            dependency(dep, expand)?;
                        }
                        if !expand && list.as_array().unwrap().is_empty() {
                            model.as_object_mut().unwrap().remove(key);
                        }
                    }
                }
                if expand {
                    model
                        .as_object_mut()
                        .unwrap()
                        .entry("relocation")
                        .or_insert(Value::Null);
                }
                if let Some(dep) = model.get_mut("relocation") {
                    if !dep.is_null() {
                        dependency(dep, expand)?;
                    } else if !expand {
                        model.as_object_mut().unwrap().remove("relocation");
                    }
                }
            }
        }
    }
    let matrix = value["request"].get("environments").is_some();
    let resolution = value.get_mut("resolution").context("Missing resolution")?;
    let groups: Vec<&mut Value> = if matrix {
        fields(resolution, &["environments"])?;
        resolution["environments"]
            .as_object_mut()
            .context("environments")?
            .values_mut()
            .collect()
    } else {
        vec![resolution]
    };
    for group in groups {
        fields(group, &["classpaths"])?;
        for cp in group["classpaths"]
            .as_object_mut()
            .context("classpaths")?
            .values_mut()
        {
            fields(cp, &["dependencies", "artifacts", "edges"])?;
            for dep in cp["dependencies"]
                .as_array_mut()
                .context("resolved dependencies")?
            {
                dependency(dep, expand)?;
            }
            for a in cp["artifacts"].as_array().context("selected artifacts")? {
                fields(a, &["id", "scope"])?;
            }
            for e in cp["edges"].as_array().context("edges")? {
                fields(e, &["from", "to", "scope"])?;
            }
        }
    }
    Ok(())
}

fn selected(value: &Value) -> Result<Vec<String>> {
    let groups: Vec<&Value> = if value["request"].get("environments").is_some() {
        value["resolution"]["environments"]
            .as_object()
            .context("environments")?
            .values()
            .collect()
    } else {
        vec![&value["resolution"]]
    };
    let mut ids = Vec::new();
    for group in groups {
        for cp in group["classpaths"]
            .as_object()
            .context("classpaths")?
            .values()
        {
            for a in cp["artifacts"].as_array().context("artifacts")? {
                ids.push(a["id"].as_str().context("artifact id")?.to_owned());
            }
        }
    }
    Ok(ids)
}

/// Expand and validate before any consumer can use a compact document.
pub fn decode(mut value: Value) -> Result<Value> {
    ensure!(value["version"] == VERSION, "{UPGRADE}");
    fields(
        &value,
        &[
            "version",
            "kind",
            "resolver",
            "request",
            "requestHash",
            "provenance",
            "contextModels",
            "resolution",
            "classpathInputs",
        ],
    )?;
    let matrix = value["request"].get("environments").is_some();
    ensure!(
        value["kind"] == if matrix { "matrix" } else { "single" },
        "Lock kind/declarations mismatch"
    );
    value.as_object_mut().unwrap().remove("kind");
    let repos = value["request"]["repositories"]
        .as_array()
        .context("repositories")?
        .clone();
    for (path, source) in value["provenance"].as_object_mut().context("provenance")? {
        fields(source, &["repository", "sha256"])?;
        let index = source["repository"]
            .as_u64()
            .context("Invalid repository reference")?;
        let repo = usize::try_from(index)
            .ok()
            .and_then(|i| repos.get(i))
            .and_then(Value::as_str)
            .context("Invalid repository reference")?;
        *source = json!({"repository":repo, "sha256":source["sha256"],
            "url":format!("{}/{path}",repo.trim_end_matches('/'))});
    }
    models_and_resolution(&mut value, true)?;
    let mut artifacts = Map::new();
    for id in selected(&value)? {
        let path = jix_resolver::Dependency::from_id(&id)?.artifact_path();
        let source = value["provenance"]
            .get(&path)
            .context("Missing artifact provenance")?;
        artifacts.insert(id, json!({"path":path,"source":source}));
    }
    value["artifacts"] = Value::Object(artifacts);
    validate(&value)?;
    // Canonical serde defaults make equivalent spellings compare equal during reconciliation.
    if matrix {
        Ok(serde_json::to_value(serde_json::from_value::<
            crate::matrix::MatrixLock,
        >(value)?)?)
    } else {
        Ok(serde_json::to_value(serde_json::from_value::<
            crate::lock::Lockfile,
        >(value)?)?)
    }
}

fn validate(value: &Value) -> Result<()> {
    if value["request"].get("environments").is_some() {
        let lock: crate::matrix::MatrixLock = serde_json::from_value(value.clone())?;
        lock.validate(&lock.spec)?;
    } else {
        let lock: crate::lock::Lockfile = serde_json::from_value(value.clone())?;
        crate::lock::validate(&lock, &lock.spec)?;
    }
    Ok(())
}

/// Compact only validated canonical locks; never silently discard contradictory evidence.
pub fn encode(mut value: Value) -> Result<Value> {
    validate(&value)?;
    let matrix = value["request"].get("environments").is_some();
    value["kind"] = json!(if matrix { "matrix" } else { "single" });
    let repos = value["request"]["repositories"]
        .as_array()
        .context("repositories")?
        .clone();
    for source in value["provenance"]
        .as_object_mut()
        .context("provenance")?
        .values_mut()
    {
        let index = repos
            .iter()
            .position(|r| r == &source["repository"])
            .context("Undeclared repository")?;
        *source = json!({"repository":index,"sha256":source["sha256"]});
    }
    value.as_object_mut().unwrap().remove("artifacts");
    models_and_resolution(&mut value, false)?;
    Ok(value)
}

/// Stable sections with compact leaf records, rather than minified whole documents.
pub fn format(value: &Value) -> Result<String> {
    fn render(v: &Value, depth: usize) -> Result<String> {
        let indent = "  ".repeat(depth);
        let next = "  ".repeat(depth + 1);
        match v {
            Value::Object(m)
                if !m.is_empty()
                    && !(m.contains_key("module")
                        || m.contains_key("sha256")
                        || (m.contains_key("id") && m.contains_key("scope"))
                        || m.contains_key("from")) =>
            {
                let sorted: BTreeMap<_, _> = m.iter().collect();
                let entries = sorted
                    .into_iter()
                    .map(|(k, v)| {
                        Ok(format!(
                            "{next}{}: {}",
                            serde_json::to_string(k)?,
                            render(v, depth + 1)?
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(format!("{{\n{}\n{indent}}}", entries.join(",\n")))
            }
            Value::Array(a) if !a.is_empty() => {
                let entries = a
                    .iter()
                    .map(|v| Ok(format!("{next}{}", render(v, depth + 1)?)))
                    .collect::<Result<Vec<_>>>()?;
                Ok(format!("[\n{}\n{indent}]", entries.join(",\n")))
            }
            _ => Ok(serde_json::to_string(v)?),
        }
    }
    Ok(render(value, 0)? + "\n")
}

pub fn read(path: &std::path::Path) -> Result<Value> {
    decode(serde_json::from_slice(&std::fs::read(path)?)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dependency_encoding_preserves_raw_model_distinctions() {
        for kind in ["jar", "", "test-jar", "ejb-client", "java-source"] {
            for version in ["", "1.0", "[1:2,)", "a%3Ab", "${x:y}"] {
                for optional_value in [None, Some(false), Some(true)] {
                    let mut original = json!({"group":"t","artifact":"a","version":version,
                        "type":kind,"classifier":"","scope":"","optional":false,"exclusions":[]});
                    if let Some(v) = optional_value {
                        original["optionalValue"] = json!(v);
                    }
                    let mut encoded = original.clone();
                    dependency(&mut encoded, false).unwrap();
                    assert_eq!(encoded.get("optionalValue"), original.get("optionalValue"));
                    dependency(&mut encoded, true).unwrap();
                    assert_eq!(encoded, original);
                }
            }
        }
        let mut explicit = json!({"group":"t","artifact":"a","version":"1","type":"jar",
            "classifier":"tests","scope":"runtime","optional":true,"exclusions":["x:*"]});
        let original = explicit.clone();
        dependency(&mut explicit, false).unwrap();
        dependency(&mut explicit, true).unwrap();
        assert_eq!(explicit, original);
    }

    #[test]
    fn fixture_roundtrip_and_malformed_documents() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("t/a/1");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a-1.pom"), "<project><modelVersion>4.0.0</modelVersion><groupId>t</groupId><artifactId>a</artifactId><version>1</version></project>").unwrap();
        std::fs::write(dir.join("a-1.jar"), b"jar").unwrap();
        let spec = serde_json::from_value(json!({"repositories":[format!("file://{}",tmp.path().display())],
            "classpaths":{"main":{"dependencies":["t:a:1"]},"explicit":{"dependencies":["t:a:1"],"resolutionContext":{}}}})).unwrap();
        let lock = crate::lock::create(&spec).unwrap();
        let canonical = serde_json::to_value(lock).unwrap();
        let disk = encode(canonical.clone()).unwrap();
        assert_eq!(decode(disk.clone()).unwrap(), canonical);
        assert_eq!(
            serde_json::from_str::<Value>(&format(&disk).unwrap()).unwrap(),
            disk
        );
        assert!(
            format(&disk)
                .unwrap()
                .contains("\"repository\":0,\"sha256\":")
        );
        assert!(disk.get("artifacts").is_none());
        for version in [1, 2, 3, 4, 6] {
            let mut broken = disk.clone();
            broken["version"] = json!(version);
            assert!(
                decode(broken)
                    .unwrap_err()
                    .to_string()
                    .contains("move jix.lock aside")
            );
        }
        for bad in [json!(-1), json!(1), json!("0"), json!(0.5)] {
            let mut broken = disk.clone();
            broken["provenance"]["t/a/1/a-1.jar"]["repository"] = bad;
            assert!(decode(broken).is_err());
        }
        let mut broken = disk.clone();
        broken["provenance"]["t/a/1/a-1.jar"]["sha256"] = json!("bad");
        assert!(decode(broken).is_err());
        let mut broken = disk.clone();
        broken["provenance"]
            .as_object_mut()
            .unwrap()
            .remove("t/a/1/a-1.jar");
        assert!(decode(broken).is_err());
        let mut broken = disk.clone();
        broken["kind"] = json!("matrix");
        assert!(decode(broken).is_err());
        let mut broken = disk.clone();
        broken["unknown"] = json!(true);
        assert!(decode(broken).is_err());
        let mut broken = disk.clone();
        broken["resolution"]["classpaths"]["main"]["dependencies"][0]["unknown"] = json!(true);
        assert!(decode(broken).is_err());
        let mut broken = disk;
        broken["classpathInputs"]["main"]["contextHash"] = json!("bad");
        assert!(decode(broken).is_err());
    }
    #[test]
    fn model_defaults_relocation_and_empty_lock_normalize_losslessly() {
        let dep = json!({"group":"unused:group","artifact":"a%name","version":"","type":"",
            "classifier":"","scope":"","optional":false,"optionalValue":false,"exclusions":[]});
        let mut value = json!({"request":{}, "resolution":{"classpaths":{}},
            "contextModels":{"context":{"pom":{"dependencies":[],"management":[dep.clone()],"relocation":dep}}}});
        let original = value.clone();
        models_and_resolution(&mut value, false).unwrap();
        assert!(
            value["contextModels"]["context"]["pom"]
                .get("dependencies")
                .is_none()
        );
        models_and_resolution(&mut value, true).unwrap();
        assert_eq!(value, original);
        let spec = serde_json::from_value(
            json!({"repositories":["https://repo.maven.apache.org/maven2"],"classpaths":{}}),
        )
        .unwrap();
        let canonical = serde_json::to_value(crate::lock::create(&spec).unwrap()).unwrap();
        let mut disk = encode(canonical.clone()).unwrap();
        disk["contextModels"] = json!({});
        disk["classpathInputs"] = json!({});
        assert_eq!(decode(disk).unwrap(), canonical);
    }
}
