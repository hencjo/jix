//! Offline construction operations shared by Nix and explicit local callers.
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    #[default]
    Maven,
    Library,
}
fn is_maven(kind: &EntryKind) -> bool {
    *kind == EntryKind::Maven
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Entry {
    #[serde(default, skip_serializing_if = "is_maven")]
    pub kind: EntryKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jar_name: Option<String>,
    pub id: String,
    pub key: String,
    pub version: String,
    pub path: PathBuf,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Classpath {
    entries: Vec<Entry>,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum SourceRoots {
    One(PathBuf),
    Many(Vec<PathBuf>),
}
impl SourceRoots {
    fn paths(&self) -> &[PathBuf] {
        match self {
            Self::One(path) => std::slice::from_ref(path),
            Self::Many(paths) => paths,
        }
    }
}

/// Validate source roots without following source symlinks and collect Java files
/// in declaration order, with deterministic traversal within each root.
pub(crate) fn java_sources(roots: &[PathBuf]) -> Result<Vec<(String, PathBuf)>> {
    let mut canonical = Vec::<PathBuf>::new();
    let mut names = BTreeMap::<String, PathBuf>::new();
    let mut sources = Vec::new();
    for root in roots {
        ensure!(
            fs::symlink_metadata(root)?.is_dir(),
            "Source root must be a directory, not a symlink: {}",
            root.display()
        );
        let resolved = root.canonicalize()?;
        ensure!(
            !canonical
                .iter()
                .any(|old| old.starts_with(&resolved) || resolved.starts_with(old)),
            "Overlapping or repeated source root: {}",
            root.display()
        );
        canonical.push(resolved);
        let mut files = Vec::new();
        tree(root, "", &mut files)?;
        for (relative, path) in files
            .into_iter()
            .filter(|(name, _)| name.ends_with(".java"))
        {
            if let Some(old) = names.insert(relative.clone(), path.clone()) {
                bail!(
                    "Duplicate Java source {relative}: {} and {}",
                    old.display(),
                    path.display()
                );
            }
            sources.push((relative, path));
        }
    }
    ensure!(
        sources
            .iter()
            .filter(|(_, path)| path
                .file_name()
                .is_some_and(|name| name == "module-info.java"))
            .count()
            <= 1,
        "Multiple module-info.java files in one compilation"
    );
    Ok(sources)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Compile {
    sources: SourceRoots,
    javac: PathBuf,
    #[serde(default)]
    classpaths: Vec<PathBuf>,
    #[serde(default)]
    javac_options: Vec<String>,
    #[serde(default)]
    modulepaths: Vec<PathBuf>,
    release: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Resource {
    from: PathBuf,
    #[serde(default)]
    into: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Jar {
    classes: PathBuf,
    #[serde(default)]
    resources: Vec<Resource>,
    #[serde(default)]
    manifest: BTreeMap<String, String>,
}

pub fn local(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.into()
    } else {
        base.join(path)
    }
}
pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    serde_json::from_slice(&fs::read(path).with_context(|| format!("Reading {}", path.display()))?)
        .context("Invalid JSON input")
}
pub fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}
pub fn entries(base: &Path, values: Vec<Entry>) -> Result<Vec<Entry>> {
    let mut chosen: BTreeMap<(EntryKind, String), Entry> = BTreeMap::new();
    let mut output = Vec::new();
    for mut entry in values {
        match entry.kind {
            EntryKind::Maven => {
                let d = jix_resolver::Dependency::from_id(&entry.id)?;
                ensure!(
                    d.key() == entry.key && d.version == entry.version && entry.jar_name.is_none(),
                    "Inconsistent classpath identity: {}",
                    entry.id
                );
            }
            EntryKind::Library => {
                ensure!(
                    !entry.id.is_empty() && entry.id == entry.key && entry.version.is_empty(),
                    "Invalid library identity: {}",
                    entry.id
                );
                stable_jar_name(
                    entry
                        .jar_name
                        .as_deref()
                        .context("Library requires jarName")?,
                )?;
            }
        }
        entry.path = local(base, &entry.path);
        ensure!(
            entry.path.is_file(),
            "Missing classpath artifact: {}",
            entry.path.display()
        );
        if let Some(previous) = chosen.get(&(entry.kind.clone(), entry.key.clone())) {
            ensure!(
                previous == &entry,
                "Conflicting artifacts: {} and {}",
                previous.id,
                entry.id
            );
        } else {
            chosen.insert((entry.kind.clone(), entry.key.clone()), entry.clone());
            output.push(entry);
        }
    }
    Ok(output)
}
pub fn classpath(values: Vec<Entry>, output: &Path) -> Result<()> {
    fs::create_dir(output)?;
    fs::create_dir(output.join("jars"))?;
    for (i, entry) in values.iter().enumerate() {
        let name = entry
            .path
            .file_name()
            .context("Artifact has no filename")?
            .to_string_lossy();
        std::os::unix::fs::symlink(
            &entry.path,
            output.join("jars").join(format!("{i:05}-{name}")),
        )?;
    }
    let joined = std::env::join_paths(values.iter().map(|e| &e.path))?;
    fs::write(output.join("classpath"), joined.as_encoded_bytes())?;
    write_json(&output.join("entries.json"), &values)
}

fn stable_jar_name(name: &str) -> Result<()> {
    ensure!(
        name.ends_with(".jar")
            && name.len() > 4
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)),
        "Invalid stable JAR filename: {name}"
    );
    Ok(())
}
pub fn modulepath(values: Vec<Entry>, output: &Path) -> Result<()> {
    fs::create_dir(output)?;
    fs::create_dir(output.join("jars"))?;
    let mut names = BTreeSet::new();
    for entry in &values {
        let name = match entry.kind {
            EntryKind::Library => entry.jar_name.clone().context("Library requires jarName")?,
            EntryKind::Maven => {
                let path = jix_resolver::Dependency::from_id(&entry.id)?.artifact_path();
                Path::new(&path)
                    .file_name()
                    .context("Missing artifact filename")?
                    .to_string_lossy()
                    .into_owned()
            }
        };
        stable_jar_name(&name)?;
        ensure!(
            names.insert(name.clone()),
            "Module-path JAR filename collision: {name}"
        );
        std::os::unix::fs::symlink(&entry.path, output.join("jars").join(name))?;
    }
    write_json(&output.join("entries.json"), &values)
}
fn read_paths(base: &Path, paths: Vec<PathBuf>) -> Result<Vec<Entry>> {
    let mut values = Vec::new();
    for cp in paths {
        let cp = local(base, &cp).canonicalize()?;
        values.extend(entries(cp.parent().unwrap(), read_json(&cp)?)?);
    }
    entries(base, values)
}
fn separate_paths(classpath: &[Entry], modulepath: &[Entry]) -> Result<()> {
    for a in classpath {
        ensure!(
            !modulepath
                .iter()
                .any(|b| (a.kind == b.kind && a.key == b.key) || a.path == b.path),
            "Artifact appears on both classpath and module path: {}",
            a.id
        );
    }
    Ok(())
}

pub fn safe_path(path: &str) -> Result<()> {
    ensure!(
        !path.is_empty()
            && !path.starts_with('/')
            && !path.contains(['\\', '\0', ':'])
            && path.split('/').all(|p| !matches!(p, "" | "." | "..")),
        "Unsafe destination: {path}"
    );
    Ok(())
}
fn tree(path: &Path, relative: &str, files: &mut Vec<(String, PathBuf)>) -> Result<()> {
    let metadata =
        fs::symlink_metadata(path).with_context(|| format!("Reading {}", path.display()))?;
    ensure!(
        !metadata.file_type().is_symlink(),
        "Symlink source unsupported: {}",
        path.display()
    );
    if metadata.is_file() {
        files.push((relative.into(), path.into()));
    } else if metadata.is_dir() {
        let mut children = fs::read_dir(path)?.collect::<std::io::Result<Vec<_>>>()?;
        children.sort_by_key(|e| e.file_name());
        for child in children {
            let name = child
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("Non-UTF8 source path"))?;
            let relative = if relative.is_empty() {
                name
            } else {
                format!("{relative}/{name}")
            };
            tree(&child.path(), &relative, files)?;
        }
    } else {
        bail!(
            "Source is not a regular file or directory: {}",
            path.display()
        );
    }
    Ok(())
}
pub fn manifest(attributes: &BTreeMap<String, String>) -> Result<Vec<u8>> {
    let mut seen = BTreeSet::new();
    for (key, value) in attributes {
        ensure!(
            !key.is_empty()
                && key.len() <= 70
                && key.bytes().next().unwrap().is_ascii_alphanumeric()
                && key
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                && seen.insert(key.to_ascii_lowercase()),
            "Invalid/duplicate manifest attribute: {key}"
        );
        ensure!(
            !value.contains(['\r', '\n', '\0']) && !value.contains("/nix/store/"),
            "Invalid manifest value: {key}"
        );
        ensure!(
            !key.eq_ignore_ascii_case("Manifest-Version") || value == "1.0",
            "Manifest-Version must be 1.0"
        );
    }
    let mut fields = vec![("Manifest-Version", "1.0")];
    fields.extend(
        attributes
            .iter()
            .filter(|(k, _)| !k.eq_ignore_ascii_case("Manifest-Version"))
            .map(|(k, v)| (k.as_str(), v.as_str())),
    );
    let mut result = Vec::new();
    for (key, value) in fields {
        let mut line = format!("{key}: {value}");
        while line.len() > 72 {
            let mut size = 72;
            while !line.is_char_boundary(size) {
                size -= 1;
            }
            result.extend_from_slice(&line.as_bytes()[..size]);
            result.extend_from_slice(b"\r\n");
            line = format!(" {}", &line[size..]);
        }
        result.extend_from_slice(line.as_bytes());
        result.extend_from_slice(b"\r\n");
    }
    result.extend_from_slice(b"\r\n");
    Ok(result)
}
fn jar(config: Jar, base: &Path, output: &Path) -> Result<()> {
    let mut candidates = Vec::new();
    tree(&local(base, &config.classes), "", &mut candidates)?;
    for r in config.resources {
        let source = local(base, &r.from);
        let into = if r.into.is_empty() && source.is_file() {
            source
                .file_name()
                .context("Missing filename")?
                .to_str()
                .context("Non-UTF8 filename")?
                .to_owned()
        } else {
            r.into
        };
        if !into.is_empty() {
            safe_path(&into)?;
        }
        tree(&source, &into, &mut candidates)?;
    }
    let mut files = BTreeMap::new();
    files.insert("META-INF/MANIFEST.MF".to_owned(), None);
    for (name, path) in candidates {
        safe_path(&name)?;
        ensure!(
            !files.keys().any(|old| old == &name
                || old.starts_with(&format!("{name}/"))
                || name.starts_with(&format!("{old}/"))),
            "Archive destination collision: {name}"
        );
        files.insert(name, Some(path));
    }
    let data = manifest(&config.manifest)?;
    let file = fs::File::create(output)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .last_modified_time(zip::DateTime::default())
        .unix_permissions(0o644);
    zip.start_file("META-INF/MANIFEST.MF", options)?;
    zip.write_all(&data)?;
    for (name, path) in files {
        if let Some(path) = path {
            zip.start_file(name, options)?;
            std::io::copy(&mut fs::File::open(path)?, &mut zip)?;
        }
    }
    zip.finish()?.sync_all()?;
    Ok(())
}
pub(crate) fn allowed_option(s: &str) -> bool {
    matches!(
        s,
        "-g" | "-parameters"
            | "-Werror"
            | "-Xlint"
            | "-nowarn"
            | "-deprecation"
            | "--enable-preview"
    ) || s
        .strip_prefix("-g:")
        .is_some_and(|x| !x.is_empty() && x.bytes().all(|b| b.is_ascii_lowercase() || b == b','))
        || s.strip_prefix("-Xlint:").is_some_and(|x| {
            !x.is_empty()
                && x.bytes()
                    .all(|b| b.is_ascii_lowercase() || b",-".contains(&b))
        })
}
fn compile(config: Compile, base: &Path, output: &Path) -> Result<()> {
    ensure!(config.release >= 8, "release must be >= 8");
    ensure!(
        config.javac_options.iter().all(|o| allowed_option(o)),
        "Unsupported javac option; inputs/output/processors must be declarative"
    );
    let values = read_paths(base, config.classpaths)?;
    let modules = read_paths(base, config.modulepaths)?;
    separate_paths(&values, &modules)?;
    let roots: Vec<_> = config
        .sources
        .paths()
        .iter()
        .map(|path| local(base, path))
        .collect();
    let sources = java_sources(&roots)?;
    fs::create_dir(output)?;
    let named_module = sources.iter().any(|(_, path)| {
        path.file_name()
            .is_some_and(|name| name == "module-info.java")
    });
    ensure!(
        config.release >= 9 || (modules.is_empty() && !named_module),
        "Modular compilation requires release >= 9"
    );
    if sources.is_empty() {
        return Ok(());
    }
    let work = tempfile::tempdir()?;
    let empty = work.path().join("empty");
    fs::create_dir(&empty)?;
    let cp = if values.is_empty() {
        empty.as_os_str().to_owned()
    } else {
        std::env::join_paths(values.iter().map(|e| &e.path))?
    };
    let mut args = vec![
        "-encoding".into(),
        "UTF-8".into(),
        "--release".into(),
        config.release.to_string(),
        "-proc:none".into(),
        "-implicit:none".into(),
        "-sourcepath".into(),
        if named_module {
            std::env::join_paths(&roots)?
                .into_string()
                .map_err(|_| anyhow::anyhow!("Non-UTF8 source path"))?
        } else {
            empty.to_string_lossy().into_owned()
        },
        "-classpath".into(),
        cp.into_string()
            .map_err(|_| anyhow::anyhow!("Non-UTF8 classpath"))?,
        "-d".into(),
        output.to_string_lossy().into_owned(),
    ];
    if !modules.is_empty() {
        let module_dir = work.path().join("modules");
        modulepath(modules, &module_dir)?;
        if !named_module {
            args.extend(["--add-modules".into(), "ALL-MODULE-PATH".into()]);
        }
        args.extend([
            "--module-path".into(),
            module_dir.join("jars").to_string_lossy().into_owned(),
        ]);
    }
    args.extend(config.javac_options);
    args.extend(
        sources
            .into_iter()
            .map(|(_, p)| p.to_string_lossy().into_owned()),
    );
    ensure!(
        args.iter().all(|a| !a.contains(['\n', '\r', '\0'])),
        "Newlines/NUL in javac argument"
    );
    let content = args
        .iter()
        .map(|s| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")))
        .collect::<Vec<_>>()
        .join("\n");
    let argfile = work.path().join("javac.args");
    fs::write(&argfile, content)?;
    let status = Command::new(local(base, &config.javac))
        .arg(format!("@{}", argfile.display()))
        .env_remove("CLASSPATH")
        .env_remove("JAVA_TOOL_OPTIONS")
        .env_remove("JDK_JAVA_OPTIONS")
        .env_remove("_JAVA_OPTIONS")
        .status()?;
    ensure!(status.success(), "javac failed: {status}");
    Ok(())
}

pub fn run(mode: &str, config: &Path, output: &Path) -> Result<()> {
    let config = config.canonicalize()?;
    let base = config.parent().unwrap();
    let output = local(&std::env::current_dir()?, output);
    ensure!(
        !output.try_exists()? && fs::symlink_metadata(&output).is_err(),
        "Output already exists: {}",
        output.display()
    );
    let temp = tempfile::tempdir_in(output.parent().context("Missing output parent")?)?;
    let staged = temp.path().join("output");
    match mode {
        "classpath" => {
            let c: Classpath = read_json(&config)?;
            classpath(entries(base, c.entries)?, &staged)?;
        }
        "modulepath" => {
            let c: Classpath = read_json(&config)?;
            modulepath(entries(base, c.entries)?, &staged)?;
        }
        "fat-jar" => crate::fat::assemble(read_json(&config)?, base, &staged)?,
        "compile" => compile(read_json(&config)?, base, &staged)?,
        "jar" => jar(read_json(&config)?, base, &staged)?,
        _ => bail!("Unknown build operation: {mode}"),
    }
    publish(&staged, &output)?;
    Ok(())
}

/// Publish without replacing a destination created concurrently.
pub fn publish(source: &Path, destination: &Path) -> Result<()> {
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        source,
        rustix::fs::CWD,
        destination,
        rustix::fs::RenameFlags::NOREPLACE,
    )?;
    Ok(())
}

#[cfg(test)]
mod source_tests {
    use super::*;

    #[test]
    fn declaration_order_then_sorted_traversal() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        fs::create_dir_all(first.join("nested")).unwrap();
        fs::create_dir(&second).unwrap();
        for file in ["Z.java", "A.java", "nested/C.java", "resource.txt"] {
            fs::write(first.join(file), "").unwrap();
        }
        fs::write(second.join("B.java"), "").unwrap();
        let paths = java_sources(&[second, first]).unwrap();
        assert_eq!(
            paths
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["B.java", "A.java", "Z.java", "nested/C.java"]
        );
    }
}
