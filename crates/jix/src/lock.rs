use crate::repository::{ArtifactSource, Repository, sha256};
use jix_resolver::{
    COMPATIBILITY_TARGET, ContextualModels, Dependency, DependencySpec, Error, ErrorKind,
    RepositoryProvider, ResolutionResult, Result, context_hash, resolve_with_contextual_models,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::Path,
};

pub const LOCK_VERSION: u32 = 5;
pub const RESOLVER_ID: &str = "maven-3.9.11/resolver-1.9.24/jix-strict-v2";
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Lockfile {
    pub version: u32,
    pub resolver: String,
    #[serde(rename = "requestHash")]
    pub spec_hash: String,
    #[serde(rename = "request")]
    pub spec: DependencySpec,
    pub artifacts: BTreeMap<String, LockedArtifact>,
    pub provenance: BTreeMap<String, ArtifactSource>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub context_models: ContextualModels,
    pub resolution: ResolutionResult,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub classpath_inputs: BTreeMap<String, ClasspathInputs>,
}
/// Exact provenance/model ownership permits safe reuse and pruning.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClasspathInputs {
    #[serde(
        default,
        rename = "contextHash",
        skip_serializing_if = "Option::is_none"
    )]
    pub context_hash: Option<String>,
    pub provenance: BTreeSet<String>,
    pub models: BTreeSet<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LockedArtifact {
    pub path: String,
    pub source: ArtifactSource,
}
fn invalid(e: impl std::fmt::Display) -> Error {
    Error::new(ErrorKind::Invalid, e.to_string())
}
fn io(e: impl std::fmt::Display) -> Error {
    Error::new(ErrorKind::Io, e.to_string())
}

/// Hash canonical, normalized JSON (sorted keys; list ordering preserved).
pub fn spec_hash(request: &DependencySpec) -> Result<String> {
    let value = serde_json::to_value(request.normalized()).map_err(invalid)?;
    Ok(sha256(&serde_json::to_vec(&value).map_err(invalid)?))
}
pub fn read_spec(path: &Path) -> Result<DependencySpec> {
    let request: DependencySpec =
        serde_json::from_slice(&std::fs::read(path).map_err(io)?).map_err(invalid)?;
    for name in request.classpaths.keys() {
        if matches!(name.as_str(), "" | "." | "..")
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return Err(invalid(
                "Classpath names must contain only ASCII letters, digits, '.', '_' or '-'",
            ));
        }
    }
    Ok(request.normalized())
}
struct TrackedRepository<'a> {
    inner: &'a mut Repository,
    paths: BTreeSet<String>,
}
impl RepositoryProvider for TrackedRepository<'_> {
    fn read(&mut self, path: &str) -> Result<Option<Vec<u8>>> {
        let bytes = self.inner.read(path)?;
        if bytes.is_some() {
            self.paths.insert(path.into());
        }
        Ok(bytes)
    }
}
fn resolve_spec(request: &DependencySpec, repository: &mut Repository) -> Result<Lockfile> {
    let mut repository = TrackedRepository {
        inner: repository,
        paths: BTreeSet::new(),
    };
    let (resolution, context_models) = resolve_with_contextual_models(request, &mut repository)?;
    let mut artifacts = BTreeMap::new();
    for cp in resolution.classpaths.values() {
        for selected in &cp.artifacts {
            if artifacts.contains_key(&selected.id) {
                continue;
            }
            let coordinate = Dependency::from_id(&selected.id)?;
            if coordinate.extension() != "jar" {
                return Err(Error::new(
                    ErrorKind::Unsupported,
                    format!("Classpath artifact is not a JAR: {}", selected.id),
                ));
            }
            let path = coordinate.artifact_path();
            repository.read(&path)?.ok_or_else(|| {
                Error::new(ErrorKind::Missing, format!("Missing artifact: {path}"))
            })?;
            artifacts.insert(
                selected.id.clone(),
                LockedArtifact {
                    path: path.clone(),
                    source: repository.inner.sources[&path].clone(),
                },
            );
        }
    }
    debug_assert!(RESOLVER_ID.starts_with(COMPATIBILITY_TARGET));
    Ok(Lockfile {
        version: LOCK_VERSION,
        resolver: RESOLVER_ID.into(),
        spec_hash: spec_hash(request)?,
        spec: request.clone(),
        artifacts,
        provenance: repository
            .paths
            .iter()
            .map(|path| (path.clone(), repository.inner.sources[path].clone()))
            .collect(),
        context_models,
        resolution,
        classpath_inputs: BTreeMap::new(),
    })
}
/// Create a new lock with independently captured classpath ownership.
pub fn create(spec: &DependencySpec) -> Result<Lockfile> {
    reconcile(spec, None)
}

/// Reconcile declarations without contacting repositories for preserved classpaths.
pub fn reconcile(spec: &DependencySpec, old: Option<&Lockfile>) -> Result<Lockfile> {
    let normalized = spec.normalized();
    let spec = &normalized;
    if let Some(old) = old {
        validate(old, &old.spec)?;
        if old.version == LOCK_VERSION && old.spec_hash == spec_hash(spec)? {
            return Ok(old.clone());
        }
    }
    let mut result = Lockfile {
        version: LOCK_VERSION,
        resolver: RESOLVER_ID.into(),
        spec_hash: spec_hash(spec)?,
        spec: spec.clone(),
        artifacts: BTreeMap::new(),
        provenance: BTreeMap::new(),
        context_models: BTreeMap::new(),
        resolution: ResolutionResult {
            classpaths: BTreeMap::new(),
        },
        classpath_inputs: BTreeMap::new(),
    };
    let mut repository = None;
    for (name, cp_spec) in &spec.classpaths {
        let preserved = old
            .filter(|old| old.version == LOCK_VERSION && old.spec.repositories == spec.repositories)
            .filter(|old| {
                old.spec.classpaths.get(name).is_some_and(|cp| {
                    serde_json::to_value(cp).ok() == serde_json::to_value(cp_spec).ok()
                })
            });
        let fresh;
        let part = if let Some(old) = preserved {
            old
        } else {
            let single = DependencySpec {
                resolution_context: None,
                repositories: spec.repositories.clone(),
                classpaths: BTreeMap::from([(name.clone(), cp_spec.clone())]),
            };
            if repository.is_none() {
                repository = Some(Repository::new(&spec.repositories)?);
            }
            fresh = resolve_spec(
                &single,
                repository.as_mut().expect("repository initialized"),
            )?;
            &fresh
        };
        let hash = context_hash(cp_spec.resolution_context.as_ref());
        let inputs = if preserved.is_some() {
            part.classpath_inputs[name].clone()
        } else {
            ClasspathInputs {
                context_hash: Some(hash.clone()),
                provenance: part.provenance.keys().cloned().collect(),
                models: part
                    .context_models
                    .get(&hash)
                    .into_iter()
                    .flat_map(|m| m.keys().cloned())
                    .collect(),
            }
        };
        let cp = &part.resolution.classpaths[name];
        for selected in &cp.artifacts {
            merge(
                &mut result.artifacts,
                &selected.id,
                &part.artifacts[&selected.id],
            )?;
        }
        for path in &inputs.provenance {
            merge(&mut result.provenance, path, &part.provenance[path])?;
        }
        if !inputs.models.is_empty() {
            let models = result.context_models.entry(hash.clone()).or_default();
            for path in &inputs.models {
                merge(models, path, &part.context_models[&hash][path])?;
            }
        }
        result
            .resolution
            .classpaths
            .insert(name.clone(), cp.clone());
        result.classpath_inputs.insert(name.clone(), inputs);
    }
    validate(&result, spec)?;
    Ok(result)
}
fn merge<T: Serialize + Clone>(map: &mut BTreeMap<String, T>, key: &str, value: &T) -> Result<()> {
    if let Some(previous) = map.get(key) {
        if serde_json::to_value(previous).map_err(invalid)?
            != serde_json::to_value(value).map_err(invalid)?
        {
            return Err(invalid(format!(
                "Conflicting shared artifact/model provenance: {key}; refusing to replace locked inputs"
            )));
        }
    } else {
        map.insert(key.into(), value.clone());
    }
    Ok(())
}

/// Replace only after a complete lock has been produced; failed resolution leaves the old file intact.
pub fn write(lock: &impl Serialize, path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut output = tempfile::NamedTempFile::new_in(parent).map_err(io)?;
    let value = serde_json::to_value(lock).map_err(invalid)?;
    let compact = crate::lock_codec::encode(value).map_err(invalid)?;
    output
        .write_all(
            crate::lock_codec::format(&compact)
                .map_err(invalid)?
                .as_bytes(),
        )
        .map_err(io)?;
    output.as_file().sync_all().map_err(io)?;
    output.persist(path).map_err(io)?;
    Ok(())
}
pub fn read(path: &Path) -> Result<Lockfile> {
    serde_json::from_value(crate::lock_codec::read(path).map_err(invalid)?).map_err(invalid)
}
/// Offline structural validation. No resolver or network operation.
pub fn validate(lock: &Lockfile, request: &DependencySpec) -> Result<()> {
    let normalized = request.normalized();
    let request = &normalized;
    if lock.version != LOCK_VERSION || lock.resolver != RESOLVER_ID {
        return Err(invalid(crate::lock_codec::UPGRADE));
    }
    if lock.spec_hash != spec_hash(request)? || lock.spec_hash != spec_hash(&lock.spec)? {
        return Err(invalid(
            "Stale lockfile: dependency declarations changed; run jix lock",
        ));
    }
    if request
        .classpaths
        .keys()
        .ne(lock.resolution.classpaths.keys())
    {
        return Err(invalid("Lockfile classpaths do not match request"));
    }
    for (id, artifact) in &lock.artifacts {
        let coordinate = Dependency::from_id(id)?;
        if coordinate.artifact_path() != artifact.path || coordinate.extension() != "jar" {
            return Err(invalid("Invalid locked artifact path"));
        }
        validate_source(&artifact.source, request)?;
        if artifact.source.url
            != format!(
                "{}/{}",
                artifact.source.repository.trim_end_matches('/'),
                artifact.path
            )
        {
            return Err(invalid("Artifact URL/path mismatch"));
        }
        if lock.provenance.get(&artifact.path) != Some(&artifact.source) {
            return Err(invalid("Missing artifact provenance"));
        }
    }
    for cp in lock.resolution.classpaths.values() {
        let mut seen = BTreeMap::new();
        for selected in &cp.artifacts {
            if !lock.artifacts.contains_key(&selected.id) {
                return Err(invalid("Classpath references missing locked artifact"));
            }
            let d = Dependency::from_id(&selected.id)?;
            if seen.insert(d.key(), d.version).is_some() {
                return Err(invalid("Duplicate artifact identity in locked classpath"));
            }
        }
    }
    for (path, source) in &lock.provenance {
        validate_source(source, request)?;
        if path.starts_with('/')
            || path
                .split('/')
                .any(|s| s.is_empty() || s == "." || s == "..")
            || source.url != format!("{}/{path}", source.repository.trim_end_matches('/'))
        {
            return Err(invalid("Invalid provenance path"));
        }
    }
    {
        if lock
            .classpath_inputs
            .keys()
            .ne(lock.resolution.classpaths.keys())
        {
            return Err(invalid(
                "Classpath ownership does not match locked classpaths",
            ));
        }
        let mut provenance = BTreeSet::new();
        let mut contextual = BTreeMap::<String, BTreeSet<String>>::new();
        let mut artifacts = BTreeSet::new();
        for (name, inputs) in &lock.classpath_inputs {
            let expected = context_hash(request.classpaths[name].resolution_context.as_ref());
            if inputs.context_hash.as_deref() != Some(&expected) {
                return Err(invalid("Classpath context fingerprint mismatch"));
            }
            let owned_models = lock.context_models.get(&expected);
            if !inputs
                .provenance
                .iter()
                .all(|p| lock.provenance.contains_key(p))
                || !inputs.models.iter().all(|p| {
                    owned_models.is_some_and(|m| m.contains_key(p)) && inputs.provenance.contains(p)
                })
            {
                return Err(invalid("Missing owned provenance/model"));
            }
            for selected in &lock.resolution.classpaths[name].artifacts {
                if !inputs
                    .provenance
                    .contains(&lock.artifacts[&selected.id].path)
                {
                    return Err(invalid("Classpath artifact lacks provenance ownership"));
                }
                artifacts.insert(selected.id.clone());
            }
            provenance.extend(inputs.provenance.iter().cloned());
            if !inputs.models.is_empty() {
                contextual
                    .entry(expected)
                    .or_default()
                    .extend(inputs.models.iter().cloned());
            }
        }
        if provenance != lock.provenance.keys().cloned().collect()
            || (contextual
                != lock
                    .context_models
                    .iter()
                    .map(|(h, m)| (h.clone(), m.keys().cloned().collect()))
                    .collect())
            || artifacts != lock.artifacts.keys().cloned().collect()
        {
            return Err(invalid("Unowned lock entries"));
        }
    }
    Ok(())
}
fn validate_source(source: &ArtifactSource, request: &DependencySpec) -> Result<()> {
    if !request.repositories.contains(&source.repository) {
        return Err(invalid("Undeclared artifact repository"));
    }
    crate::repository::validate_url(&source.repository)?;
    if source.sha256.len() != 64
        || !source
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid("Invalid SHA-256 hash"));
    }
    Ok(())
}
