//! Deterministic executable classpath archives. No resolution or Nix calls.
use crate::build::{Entry, entries, local, manifest, safe_path};
use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Relocation {
    from: String,
    to: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    entries: Vec<Entry>,
    main_class: String,
    #[serde(default)]
    relocations: Vec<Relocation>,
    #[serde(default)]
    relocate_string_constants: bool,
    relocator: Option<PathBuf>,
}
fn package(s: &str) -> bool {
    !s.is_empty()
        && s.split('.').all(|p| {
            !p.is_empty()
                && (p.as_bytes()[0].is_ascii_alphabetic() || b"_$".contains(&p.as_bytes()[0]))
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_$".contains(&b))
        })
}
fn relocate(path: &str, rules: &[Relocation]) -> String {
    for rule in rules {
        if let Some(suffix) = path.strip_prefix(&(rule.from.clone() + "/")) {
            return format!("{}/{suffix}", rule.to);
        }
    }
    path.into()
}
fn insert(files: &mut BTreeMap<String, Vec<u8>>, name: String, bytes: Vec<u8>) -> Result<()> {
    safe_path(&name)?;
    let prefix = format!("{name}/");
    ensure!(
        !files
            .range(prefix.clone()..)
            .next()
            .is_some_and(|(old, _)| old.starts_with(&prefix)),
        "Archive path collision: {name}"
    );
    let mut parent = name.as_str();
    while let Some((head, _)) = parent.rsplit_once('/') {
        ensure!(!files.contains_key(head), "Archive path collision: {name}");
        parent = head;
    }
    if let Some(previous) = files.get(&name) {
        ensure!(previous == &bytes, "Conflicting archive entry: {name}");
    } else {
        files.insert(name, bytes);
    }
    Ok(())
}
pub fn assemble(mut config: Config, base: &Path, output: &Path) -> Result<()> {
    ensure!(package(&config.main_class), "Invalid main class");
    let mut from = BTreeSet::new();
    for rule in &mut config.relocations {
        ensure!(
            package(&rule.from)
                && package(&rule.to)
                && rule.from != rule.to
                && from.insert(rule.from.clone()),
            "Invalid or duplicate relocation rule"
        );
        rule.from = rule.from.replace('.', "/");
        rule.to = rule.to.replace('.', "/");
    }
    config
        .relocations
        .sort_by(|a, b| b.from.len().cmp(&a.from.len()).then(a.from.cmp(&b.from)));
    let inputs = entries(base, config.entries)?;
    let mut files = BTreeMap::new();
    let mut services: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut origins = BTreeMap::new();
    let mut multi_release = false;
    let mut total = 0u64;
    for (index, entry) in inputs.iter().enumerate() {
        let origin = format!("{index:05}");
        origins.insert(
            origin.clone(),
            serde_json::json!({"kind":entry.kind,"id":entry.id}),
        );
        let mut jar = zip::ZipArchive::new(fs::File::open(&entry.path)?)?;
        let is_multi = if let Ok(mut file) = jar.by_name("META-INF/MANIFEST.MF") {
            ensure!(file.size() <= 1024 * 1024, "Oversized input manifest");
            let mut data = String::new();
            file.by_ref().take(1024 * 1024).read_to_string(&mut data)?;
            let unfolded = data.replace("\r\n", "\n").replace("\n ", "");
            unfolded
                .split("\n\n")
                .next()
                .unwrap_or("")
                .lines()
                .any(|l| {
                    l.split_once(':').is_some_and(|(k, v)| {
                        k.eq_ignore_ascii_case("Multi-Release")
                            && v.trim().eq_ignore_ascii_case("true")
                    })
                })
        } else {
            false
        };
        let mut seen = BTreeSet::new();
        for i in 0..jar.len() {
            let mut member = jar.by_index(i)?;
            let name = member.name().to_owned();
            safe_path(name.strip_suffix('/').unwrap_or(&name))?;
            ensure!(seen.insert(name.clone()), "Duplicate ZIP entry: {name}");
            ensure!(
                member.unix_mode().is_none_or(|m| m & 0o170000 != 0o120000),
                "ZIP symlink: {name}"
            );
            if member.is_dir() {
                continue;
            }
            let upper = name.to_ascii_uppercase();
            if upper == "META-INF/MANIFEST.MF"
                || upper == "META-INF/INDEX.LIST"
                || (upper.starts_with("META-INF/")
                    && !upper[9..].contains('/')
                    && (upper.ends_with(".SF")
                        || upper.ends_with(".RSA")
                        || upper.ends_with(".DSA")
                        || upper.ends_with(".EC")
                        || upper[9..].starts_with("SIG-")))
            {
                continue;
            }
            ensure!(
                !name.starts_with("META-INF/jix/"),
                "Reserved archive namespace: {name}"
            );
            let (prefix, logical) = if let Some(rest) = name.strip_prefix("META-INF/versions/") {
                let (version, path) = rest.split_once('/').context("Invalid multi-release path")?;
                let number: u32 = version.parse().context("Invalid multi-release version")?;
                ensure!(
                    is_multi
                        && number >= 9
                        && number.to_string() == version
                        && !path.starts_with("META-INF/"),
                    "Ambiguous multi-release entry: {name}"
                );
                (format!("META-INF/versions/{version}/"), path)
            } else {
                (String::new(), name.as_str())
            };
            if logical == "module-info.class" {
                continue;
            }
            total = total
                .checked_add(member.size())
                .context("ZIP size overflow")?;
            ensure!(
                member.size() <= 64 * 1024 * 1024 && total <= 512 * 1024 * 1024,
                "Archive exceeds extraction limits (64 MiB entry / 512 MiB total)"
            );
            let mut bytes = Vec::new();
            member
                .by_ref()
                .take(64 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() as u64 == member.size(),
                "ZIP entry size mismatch"
            );
            multi_release |= !prefix.is_empty();
            let basename = logical.rsplit('/').next().unwrap().to_ascii_uppercase();
            let legal = ["LICENSE", "NOTICE"].iter().any(|prefix| {
                basename == *prefix
                    || basename.starts_with(&format!("{prefix}."))
                    || basename.starts_with(&format!("{prefix}-"))
            });
            if legal && !logical.ends_with(".class") {
                insert(
                    &mut files,
                    format!("META-INF/jix/licenses/{origin}/{name}"),
                    bytes,
                )?;
            } else if let Some(service) = logical.strip_prefix("META-INF/services/") {
                ensure!(package(service), "Invalid service name: {service}");
                let service =
                    relocate(&service.replace('.', "/"), &config.relocations).replace('/', ".");
                let providers = services
                    .entry(format!("{prefix}META-INF/services/{service}"))
                    .or_default();
                for line in std::str::from_utf8(&bytes)?.lines() {
                    let provider = line.split('#').next().unwrap().trim();
                    if provider.is_empty() {
                        continue;
                    }
                    ensure!(package(provider), "Invalid service provider: {provider}");
                    let provider = relocate(&provider.replace('.', "/"), &config.relocations)
                        .replace('/', ".");
                    if !providers.contains(&provider) {
                        providers.push(provider);
                    }
                }
            } else {
                insert(
                    &mut files,
                    format!("{prefix}{}", relocate(logical, &config.relocations)),
                    bytes,
                )?;
            }
        }
    }
    for (name, providers) in services {
        insert(&mut files, name, (providers.join("\n") + "\n").into_bytes())?;
    }
    let mut attributes = BTreeMap::from([(
        "Main-Class".into(),
        relocate(&config.main_class.replace('.', "/"), &config.relocations).replace('/', "."),
    )]);
    if multi_release {
        attributes.insert("Multi-Release".into(), "true".into());
    }
    if !config.relocations.is_empty() {
        let work = tempfile::tempdir()?;
        let classes = work.path().join("classes");
        fs::create_dir(&classes)?;
        for (name, bytes) in &files {
            if name.ends_with(".class") {
                let path = classes.join(name);
                fs::create_dir_all(path.parent().unwrap())?;
                fs::write(path, bytes)?;
            }
        }
        let rules = work.path().join("rules");
        fs::write(
            &rules,
            config
                .relocations
                .iter()
                .map(|r| format!("{}\t{}\n", r.from, r.to))
                .collect::<String>(),
        )?;
        let helper = local(
            base,
            config
                .relocator
                .as_deref()
                .context("Relocation requires an explicit helper")?,
        );
        let status = Command::new(helper)
            .arg(&classes)
            .arg(&rules)
            .arg(config.relocate_string_constants.to_string())
            .env_remove("JAVA_TOOL_OPTIONS")
            .env_remove("JDK_JAVA_OPTIONS")
            .env_remove("_JAVA_OPTIONS")
            .status()?;
        ensure!(status.success(), "Bytecode relocation failed: {status}");
        for (name, bytes) in &mut files {
            if name.ends_with(".class") {
                *bytes = fs::read(classes.join(name))?;
            }
        }
    } else if config.relocate_string_constants {
        bail!("String relocation requires relocation rules");
    }
    insert(
        &mut files,
        "META-INF/jix/origins.json".into(),
        serde_json::to_vec_pretty(&origins)?,
    )?;
    let mut zip = zip::ZipWriter::new(fs::File::create(output)?);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .last_modified_time(zip::DateTime::default())
        .unix_permissions(0o644);
    zip.start_file("META-INF/MANIFEST.MF", options)?;
    zip.write_all(&manifest(&attributes)?)?;
    for (name, bytes) in files {
        zip.start_file(name, options)?;
        zip.write_all(&bytes)?;
    }
    zip.finish()?.sync_all()?;
    Ok(())
}
