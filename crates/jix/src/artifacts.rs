//! Locked-byte verification and portable exports; never resolve or download.
use crate::{
    build::{self, Entry},
    lock::{self, Lockfile},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<crate::matrix::Selection>,
    pub artifacts: BTreeMap<String, PathBuf>,
}
pub fn selected(lock: &Lockfile, name: Option<&str>) -> Result<BTreeSet<String>> {
    if let Some(name) = name {
        let cp = lock
            .resolution
            .classpaths
            .get(name)
            .with_context(|| format!("Unknown classpath: {name}"))?;
        Ok(cp
            .artifacts
            .iter()
            .map(|a| lock.artifacts[&a.id].path.clone())
            .collect())
    } else {
        Ok(lock.provenance.keys().cloned().collect())
    }
}
fn hash_copy(source: &Path, mut destination: Option<&mut fs::File>) -> Result<String> {
    let mut input = fs::File::open(source)?;
    ensure!(
        input.metadata()?.is_file(),
        "Not a regular artifact: {}",
        source.display()
    );
    let mut digest = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
        if let Some(ref mut out) = destination {
            out.write_all(&buffer[..n])?;
        }
    }
    Ok(format!("{:x}", digest.finalize()))
}
pub fn verify(lock: &Lockfile, manifest_path: &Path, name: Option<&str>) -> Result<()> {
    lock::validate(lock, &lock.spec)?;
    let map: Manifest = build::read_json(manifest_path)?;
    ensure!(
        map.version == 1 && map.selection.is_none(),
        "Matrix artifact manifest requires an explicit environment and matrix lock"
    );
    verify_contents(lock, manifest_path, name, &map)
}
pub fn verify_environment(
    lock: &crate::matrix::MatrixLock,
    manifest_path: &Path,
    environment: &str,
    name: Option<&str>,
) -> Result<()> {
    let selected = lock.select(environment)?;
    let map: Manifest = build::read_json(manifest_path)?;
    ensure!(
        map.version == 2 && map.selection.as_ref() == Some(&lock.selection(environment)?),
        "Artifact environment/context/lock selection mismatch"
    );
    verify_contents(&selected, manifest_path, name, &map)
}
fn verify_contents(
    lock: &Lockfile,
    manifest_path: &Path,
    name: Option<&str>,
    map: &Manifest,
) -> Result<()> {
    let base = manifest_path.canonicalize()?;
    let base = base.parent().unwrap();
    let mut failures = Vec::new();
    for path in selected(lock, name)? {
        build::safe_path(&path)?;
        let result = (|| -> Result<()> {
            let source = map
                .artifacts
                .get(&path)
                .with_context(|| format!("Missing artifact mapping: {path}"))?;
            ensure!(
                hash_copy(&build::local(base, source), None)? == lock.provenance[&path].sha256,
                "SHA-256 mismatch"
            );
            Ok(())
        })();
        if let Err(error) = result {
            failures.push(format!("{path}: {error}"));
        }
    }
    ensure!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}
pub fn materialize(input: &Path, output: &Path) -> Result<()> {
    materialize_environment(input, output, None)
}
pub fn materialize_environment(
    input: &Path,
    output: &Path,
    environment: Option<&str>,
) -> Result<()> {
    let input = input.canonicalize()?;
    let original = crate::lock_codec::read(&input.join("jix.lock"))?;
    let map: Manifest = build::read_json(&input.join("artifacts.json"))?;
    let lock = if original["request"].get("environments").is_some() {
        let matrix: crate::matrix::MatrixLock = serde_json::from_value(original.clone())?;
        let name = environment
            .or_else(|| {
                map.selection
                    .as_ref()
                    .map(|selection| selection.environment.as_str())
            })
            .context("Matrix export requires a selected artifact manifest")?;
        ensure!(
            map.version == 2 && map.selection.as_ref() == Some(&matrix.selection(name)?),
            "Artifact environment/context/lock selection mismatch"
        );
        matrix.select(name)?
    } else {
        ensure!(
            environment.is_none(),
            "--environment requires a matrix export"
        );
        ensure!(
            map.version == 1 && map.selection.is_none(),
            "Unsupported artifact manifest version"
        );
        let lock: Lockfile = serde_json::from_value(original.clone())?;
        lock::validate(&lock, &lock.spec)?;
        lock
    };
    let output = build::local(&std::env::current_dir()?, output);
    ensure!(
        fs::symlink_metadata(&output).is_err(),
        "Output already exists: {}",
        output.display()
    );
    let temp = tempfile::tempdir_in(output.parent().context("Missing export parent")?)?;
    let mut exported = Manifest {
        version: map.version,
        selection: map.selection.clone(),
        artifacts: BTreeMap::new(),
    };
    for path in selected(&lock, None)? {
        build::safe_path(&path)?;
        let source = map
            .artifacts
            .get(&path)
            .with_context(|| format!("Missing artifact mapping: {path}"))?;
        let relative = PathBuf::from("repository").join(&path);
        let dest = temp.path().join(&relative);
        fs::create_dir_all(dest.parent().unwrap())?;
        let mut file = fs::File::create(&dest)?;
        ensure!(
            hash_copy(&build::local(&input, source), Some(&mut file))?
                == lock.provenance[&path].sha256,
            "SHA-256 mismatch: {path}"
        );
        file.sync_all()?;
        exported.artifacts.insert(path, relative);
    }
    build::write_json(&temp.path().join("artifacts.json"), &exported)?;
    lock::write(&original, &temp.path().join("jix.lock"))?;
    for (name, cp) in &lock.resolution.classpaths {
        build::safe_path(name)?;
        let directory = temp.path().join("classpaths").join(name);
        fs::create_dir_all(&directory)?;
        let mut entries = Vec::new();
        for a in &cp.artifacts {
            let d = jix_resolver::Dependency::from_id(&a.id)?;
            entries.push(Entry {
                kind: Default::default(),
                jar_name: None,
                id: a.id.clone(),
                key: d.key(),
                version: d.version,
                path: PathBuf::from("../..").join(&exported.artifacts[&lock.artifacts[&a.id].path]),
            });
        }
        build::write_json(&directory.join("entries.json"), &entries)?;
        build::write_json(
            &directory.join("classpath.json"),
            &serde_json::json!({"entries":entries}),
        )?;
    }
    build::publish(temp.path(), &output)?;
    Ok(())
}
