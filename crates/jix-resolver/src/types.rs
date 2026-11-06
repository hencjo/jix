use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
    Invalid,
    Unsupported,
    Missing,
    Cycle,
    Limit,
    Io,
}

#[derive(Debug, Clone, Serialize)]
pub struct Error {
    pub kind: ErrorKind,
    pub message: String,
}
impl Error {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Invalid, message)
    }
    pub(crate) fn unsupported(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Unsupported, message)
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}
impl std::error::Error for Error {}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DependencySpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution_context: Option<ResolutionContext>,
    pub repositories: Vec<String>,
    pub classpaths: BTreeMap<String, ClasspathRequest>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClasspathRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution_context: Option<ResolutionContext>,
    #[serde(default)]
    pub scope: ClasspathScope,
    #[serde(default)]
    pub platforms: Vec<String>,
    pub dependencies: Vec<DependencyDeclaration>,
}
/// Declared Maven session properties. Never populated from the process environment.
#[derive(Debug, Default, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolutionContext {
    #[serde(default)]
    pub system_properties: BTreeMap<String, String>,
    #[serde(default)]
    pub user_properties: BTreeMap<String, String>,
}
impl ResolutionContext {
    /// Artifact descriptor requests merge user properties into system properties (MNG-7563).
    pub fn properties(&self) -> BTreeMap<String, String> {
        let mut properties = self.system_properties.clone();
        properties.extend(self.user_properties.clone());
        properties
    }
}
impl DependencySpec {
    pub fn normalized(&self) -> Self {
        let mut result = self.clone();
        for cp in result.classpaths.values_mut() {
            if cp.resolution_context.is_none() {
                cp.resolution_context = result.resolution_context.clone();
            }
        }
        result.resolution_context = None;
        result
    }
}
/// Fingerprints canonical JSON, including null for legacy conservative resolution.
pub fn context_hash(context: Option<&ResolutionContext>) -> String {
    use sha2::{Digest, Sha256};
    let canonical = serde_json::to_value(context).expect("context is JSON");
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&canonical).expect("context is JSON"))
    )
}
#[derive(Debug, Default, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ClasspathScope {
    #[default]
    Compile,
    Runtime,
    Test,
}
impl ClasspathScope {
    pub(crate) fn includes(self, scope: &str) -> bool {
        match self {
            Self::Compile => matches!(scope, "compile" | "provided" | "system"),
            Self::Runtime => matches!(scope, "compile" | "runtime"),
            Self::Test => true,
        }
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum DependencyDeclaration {
    Coordinate(String),
    Detailed(DependencyDetails),
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DependencyDetails {
    pub module: String,
    #[serde(default)]
    pub classifier: String,
    #[serde(default = "jar")]
    pub extension: String,
    #[serde(default)]
    pub exclude: Vec<String>,
}
fn jar() -> String {
    "jar".into()
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dependency {
    pub group: String,
    pub artifact: String,
    pub version: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub classifier: String,
    pub scope: String,
    pub optional: bool,
    #[serde(
        default,
        rename = "optionalValue",
        skip_serializing_if = "Option::is_none"
    )]
    pub optional_value: Option<bool>,
    pub exclusions: Vec<String>,
}
impl Dependency {
    pub fn from_spec(spec: &DependencyDeclaration) -> Result<Self> {
        let (module, kind, classifier, exclusions) = match spec {
            DependencyDeclaration::Coordinate(s) => (s.as_str(), "jar", "", Vec::new()),
            DependencyDeclaration::Detailed(d) => (
                d.module.as_str(),
                d.extension.as_str(),
                d.classifier.as_str(),
                d.exclude.clone(),
            ),
        };
        let parts: Vec<_> = module.split(':').collect();
        if !(2..=3).contains(&parts.len()) {
            return Err(Error::invalid(format!("Invalid coordinate: {module}")));
        }
        let dep = Self {
            group: parts[0].into(),
            artifact: parts[1].into(),
            version: parts.get(2).unwrap_or(&"").to_string(),
            kind: kind.into(),
            classifier: classifier.into(),
            scope: "compile".into(),
            optional: false,
            optional_value: None,
            exclusions,
        };
        dep.validate(false)?;
        Ok(dep)
    }
    pub fn from_id(id: &str) -> Result<Self> {
        let p: Vec<_> = id.split(':').collect();
        if p.len() != 5 {
            return Err(Error::invalid(
                "Expected group:artifact:extension:classifier:version",
            ));
        }
        let dep = Self {
            group: p[0].into(),
            artifact: p[1].into(),
            kind: p[2].into(),
            classifier: p[3].into(),
            version: p[4].into(),
            scope: "compile".into(),
            ..Self::default()
        };
        dep.validate(true)?;
        Ok(dep)
    }
    pub fn key(&self) -> String {
        format!(
            "{}:{}:{}:{}",
            self.group,
            self.artifact,
            self.extension(),
            self.effective_classifier()
        )
    }
    pub fn id(&self) -> String {
        format!("{}:{}", self.key(), self.version)
    }
    pub(crate) fn model_key(&self) -> String {
        format!(
            "{}:{}:{}:{}",
            self.group, self.artifact, self.kind, self.classifier
        )
    }
    pub fn extension(&self) -> &str {
        match self.kind.as_str() {
            "test-jar" | "maven-plugin" | "ejb" | "ejb-client" | "javadoc" | "java-source" => "jar",
            other => other,
        }
    }
    pub fn effective_classifier(&self) -> &str {
        if !self.classifier.is_empty() {
            return &self.classifier;
        }
        match self.kind.as_str() {
            "test-jar" => "tests",
            "ejb-client" => "client",
            "javadoc" => "javadoc",
            "java-source" => "sources",
            _ => "",
        }
    }
    pub fn pom_path(&self) -> String {
        format!(
            "{}/{}/{}/{}-{}.pom",
            self.group.replace('.', "/"),
            self.artifact,
            self.version,
            self.artifact,
            self.version
        )
    }
    pub fn artifact_path(&self) -> String {
        let suffix = if self.effective_classifier().is_empty() {
            String::new()
        } else {
            format!("-{}", self.effective_classifier())
        };
        format!(
            "{}/{}/{}/{}-{}{}.{}",
            self.group.replace('.', "/"),
            self.artifact,
            self.version,
            self.artifact,
            self.version,
            suffix,
            self.extension()
        )
    }
    pub(crate) fn validate(&self, require_version: bool) -> Result<()> {
        for part in [&self.group, &self.artifact, &self.kind] {
            validate_component(part)?;
        }
        if !self.classifier.is_empty() {
            validate_component(&self.classifier)?;
        }
        if require_version || !self.version.is_empty() {
            validate_version(&self.version)?;
        }
        for e in &self.exclusions {
            let parts: Vec<_> = e.split(':').collect();
            if parts.len() != 2 {
                return Err(Error::invalid(format!("Invalid exclusion: {e}")));
            }
            for p in parts {
                if p != "*" {
                    validate_component(p)?;
                }
            }
        }
        match self.scope.as_str() {
            "compile" | "runtime" | "provided" | "test" | "import" => (),
            "system" => {
                return Err(Error::unsupported(
                    "System-path dependencies are not supported",
                ));
            }
            _ => {
                return Err(Error::invalid(format!(
                    "Invalid dependency scope: {}",
                    self.scope
                )));
            }
        }
        Ok(())
    }
}
pub(crate) fn validate_component(s: &str) -> Result<()> {
    if s.is_empty()
        || s.split('.').any(str::is_empty)
        || !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err(Error::invalid(format!(
            "Invalid coordinate component: {s:?}"
        )));
    }
    Ok(())
}
pub(crate) fn validate_version(s: &str) -> Result<()> {
    if s.contains(['[', ']', '(', ')', ','])
        || s.ends_with("SNAPSHOT")
        || matches!(s, "LATEST" | "RELEASE")
    {
        return Err(Error::unsupported(format!(
            "Only exact release versions are supported: {s}"
        )));
    }
    validate_component(s)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedArtifact {
    pub id: String,
    pub scope: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub scope: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClasspathResult {
    pub dependencies: Vec<Dependency>,
    pub artifacts: Vec<SelectedArtifact>,
    pub edges: Vec<Edge>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolutionResult {
    pub classpaths: BTreeMap<String, ClasspathResult>,
}
