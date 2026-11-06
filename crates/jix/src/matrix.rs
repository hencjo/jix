//! Named, explicit resolution environments. The Maven engine still resolves one context at a time.
use crate::{
    lock::{self, ClasspathInputs, LockedArtifact, Lockfile},
    repository::{ArtifactSource, sha256},
};
use anyhow::{Context, Result, ensure};
use jix_resolver::{
    ClasspathRequest, ContextualModels, DependencySpec, ResolutionContext, ResolutionResult,
    context_hash,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const VERSION: u32 = 5;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Spec {
    pub repositories: Vec<String>,
    pub environments: BTreeMap<String, Environment>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Environment {
    pub resolution_context: ResolutionContext,
    pub classpaths: BTreeMap<String, ClasspathRequest>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Declaration {
    repositories: Vec<String>,
    environments: BTreeMap<String, EnvironmentDeclaration>,
    classpaths: Option<BTreeMap<String, ClasspathRequest>>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EnvironmentDeclaration {
    resolution_context: ResolutionContext,
    classpaths: Option<BTreeMap<String, ClasspathRequest>>,
}
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains("--")
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
}
fn validate_classpaths(classpaths: &BTreeMap<String, ClasspathRequest>) -> Result<()> {
    for (name, cp) in classpaths {
        ensure!(
            !name.is_empty()
                && !matches!(name.as_str(), "." | "..")
                && name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c)),
            "Invalid classpath name: {name}"
        );
        ensure!(
            cp.resolution_context.is_none(),
            "Matrix classpaths cannot declare resolutionContext; declare it on the environment"
        );
    }
    Ok(())
}
pub fn normalize(value: serde_json::Value) -> Result<Spec> {
    let declaration: Declaration = serde_json::from_value(value)?;
    if let Some(shared) = &declaration.classpaths {
        validate_classpaths(shared)?;
    }
    let mut environments = BTreeMap::new();
    for (name, env) in declaration.environments {
        let classpaths = env
            .classpaths
            .or_else(|| declaration.classpaths.clone())
            .context("Environment needs classpaths or a shared classpaths map")?;
        environments.insert(
            name,
            Environment {
                resolution_context: env.resolution_context,
                classpaths,
            },
        );
    }
    let spec = Spec {
        repositories: declaration.repositories,
        environments,
    };
    spec.validate()?;
    Ok(spec)
}
impl Spec {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.repositories.is_empty(),
            "At least one repository is required"
        );
        ensure!(
            !self.environments.is_empty(),
            "Matrix must declare at least one environment"
        );
        for (name, env) in &self.environments {
            ensure!(
                valid_name(name),
                "Invalid environment name: {name}; use letters, digits, '_' or '-' and no '--'"
            );
            validate_classpaths(&env.classpaths)?;
        }
        Ok(())
    }
    pub fn hash(&self) -> Result<String> {
        Ok(sha256(&serde_json::to_vec(&serde_json::to_value(self)?)?))
    }
    pub fn flatten(&self) -> DependencySpec {
        let mut classpaths = BTreeMap::new();
        for (name, env) in &self.environments {
            for (cp, declaration) in &env.classpaths {
                let mut declaration = declaration.clone();
                declaration.resolution_context = Some(env.resolution_context.clone());
                classpaths.insert(format!("{name}--{cp}"), declaration);
            }
        }
        DependencySpec {
            repositories: self.repositories.clone(),
            resolution_context: None,
            classpaths,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MatrixResolution {
    pub environments: BTreeMap<String, ResolutionResult>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MatrixLock {
    pub version: u32,
    pub resolver: String,
    #[serde(rename = "requestHash")]
    pub spec_hash: String,
    #[serde(rename = "request")]
    pub spec: Spec,
    pub artifacts: BTreeMap<String, LockedArtifact>,
    pub provenance: BTreeMap<String, ArtifactSource>,
    pub context_models: ContextualModels,
    pub resolution: MatrixResolution,
    pub classpath_inputs: BTreeMap<String, BTreeMap<String, ClasspathInputs>>,
}
impl MatrixLock {
    fn flatten(&self) -> Result<Lockfile> {
        let spec = self.spec.flatten();
        let mut classpaths = BTreeMap::new();
        let mut inputs = BTreeMap::new();
        for (name, result) in &self.resolution.environments {
            ensure!(valid_name(name), "Invalid locked environment: {name}");
            for (cp, value) in &result.classpaths {
                classpaths.insert(format!("{name}--{cp}"), value.clone());
            }
        }
        for (name, owned) in &self.classpath_inputs {
            ensure!(valid_name(name), "Invalid ownership environment: {name}");
            for (cp, value) in owned {
                inputs.insert(format!("{name}--{cp}"), value.clone());
            }
        }
        Ok(Lockfile {
            version: lock::LOCK_VERSION,
            resolver: self.resolver.clone(),
            spec_hash: lock::spec_hash(&spec)?,
            spec,
            artifacts: self.artifacts.clone(),
            provenance: self.provenance.clone(),
            context_models: self.context_models.clone(),
            resolution: ResolutionResult { classpaths },
            classpath_inputs: inputs,
        })
    }
    pub fn validate(&self, spec: &Spec) -> Result<()> {
        spec.validate()?;
        self.spec.validate()?;
        ensure!(
            self.version == VERSION && self.resolver == lock::RESOLVER_ID,
            "Unsupported matrix lock format/resolver"
        );
        ensure!(
            self.spec_hash == spec.hash()? && self.spec_hash == self.spec.hash()?,
            "Stale lockfile: matrix declarations changed; run jix lock"
        );
        ensure!(
            self.spec
                .environments
                .keys()
                .eq(self.resolution.environments.keys())
                && self
                    .spec
                    .environments
                    .keys()
                    .eq(self.classpath_inputs.keys()),
            "Matrix environment ownership does not match declarations"
        );
        lock::validate(&self.flatten()?, &spec.flatten())?;
        Ok(())
    }
    pub fn select(&self, name: &str) -> Result<Lockfile> {
        self.validate(&self.spec)?;
        let env = self
            .spec
            .environments
            .get(name)
            .with_context(|| format!("Unknown environment: {name}"))?;
        let mut result = self.flatten()?;
        result.spec = DependencySpec {
            repositories: self.spec.repositories.clone(),
            resolution_context: Some(env.resolution_context.clone()),
            classpaths: env.classpaths.clone(),
        }
        .normalized();
        result.spec_hash = lock::spec_hash(&result.spec)?;
        result.resolution = self.resolution.environments[name].clone();
        result.classpath_inputs = self.classpath_inputs[name].clone();
        let paths: BTreeSet<_> = result
            .classpath_inputs
            .values()
            .flat_map(|i| i.provenance.iter().cloned())
            .collect();
        let models: BTreeSet<_> = result
            .classpath_inputs
            .values()
            .flat_map(|i| i.models.iter().cloned())
            .collect();
        let artifacts: BTreeSet<_> = result
            .resolution
            .classpaths
            .values()
            .flat_map(|c| c.artifacts.iter().map(|a| a.id.clone()))
            .collect();
        result.provenance.retain(|p, _| paths.contains(p));
        result.artifacts.retain(|id, _| artifacts.contains(id));
        let hash = context_hash(Some(&env.resolution_context));
        result.context_models.retain(|h, _| h == &hash);
        for group in result.context_models.values_mut() {
            group.retain(|p, _| models.contains(p));
        }
        result.context_models.retain(|_, g| !g.is_empty());
        lock::validate(&result, &result.spec)?;
        Ok(result)
    }
    pub fn selection(&self, name: &str) -> Result<Selection> {
        let env = self
            .spec
            .environments
            .get(name)
            .with_context(|| format!("Unknown environment: {name}"))?;
        Ok(Selection {
            environment: name.into(),
            context_hash: context_hash(Some(&env.resolution_context)),
            request_hash: self.spec_hash.clone(),
        })
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Selection {
    pub environment: String,
    pub context_hash: String,
    pub request_hash: String,
}
pub fn reconcile(spec: &Spec, old: Option<&MatrixLock>) -> Result<MatrixLock> {
    spec.validate()?;
    if let Some(old) = old {
        old.validate(&old.spec)?;
    }
    let flat = old.map(MatrixLock::flatten).transpose()?;
    let resolved = lock::reconcile(&spec.flatten(), flat.as_ref())?;
    let mut environments = BTreeMap::new();
    let mut classpath_inputs = BTreeMap::new();
    for (name, env) in &spec.environments {
        let mut classpaths = BTreeMap::new();
        let mut owned = BTreeMap::new();
        for cp in env.classpaths.keys() {
            let key = format!("{name}--{cp}");
            classpaths.insert(cp.clone(), resolved.resolution.classpaths[&key].clone());
            owned.insert(cp.clone(), resolved.classpath_inputs[&key].clone());
        }
        environments.insert(name.clone(), ResolutionResult { classpaths });
        classpath_inputs.insert(name.clone(), owned);
    }
    let result = MatrixLock {
        version: VERSION,
        resolver: lock::RESOLVER_ID.into(),
        spec_hash: spec.hash()?,
        spec: spec.clone(),
        artifacts: resolved.artifacts,
        provenance: resolved.provenance,
        context_models: resolved.context_models,
        resolution: MatrixResolution { environments },
        classpath_inputs,
    };
    result.validate(spec)?;
    Ok(result)
}
pub fn resolve(spec: &Spec) -> Result<MatrixResolution> {
    spec.validate()?;
    let mut repository = crate::repository::Repository::new(&spec.repositories)?;
    let mut environments = BTreeMap::new();
    for (name, env) in &spec.environments {
        let request = DependencySpec {
            repositories: spec.repositories.clone(),
            resolution_context: Some(env.resolution_context.clone()),
            classpaths: env.classpaths.clone(),
        };
        environments.insert(
            name.clone(),
            jix_resolver::resolve(&request, &mut repository)?,
        );
    }
    Ok(MatrixResolution { environments })
}
