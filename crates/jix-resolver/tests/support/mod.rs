#![allow(dead_code)]
use jix_resolver::*;
use serde_json::json;
use std::{fs, path::PathBuf};

pub struct Fixture {
    pub directory: tempfile::TempDir,
}
impl Fixture {
    pub fn new() -> Self {
        Self {
            directory: tempfile::tempdir().unwrap(),
        }
    }
    pub fn pom(&self, artifact: &str, version: &str, body: &str) {
        self.named_pom("fixture", artifact, version, &format!("<groupId>fixture</groupId><artifactId>{artifact}</artifactId><version>{version}</version>{body}"));
    }
    pub fn named_pom(&self, group: &str, artifact: &str, version: &str, body: &str) {
        let path = self
            .directory
            .path()
            .join(group.replace('.', "/"))
            .join(artifact)
            .join(version);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join(format!("{artifact}-{version}.pom")), format!(r#"<project xmlns="http://maven.apache.org/POM/4.0.0"><modelVersion>4.0.0</modelVersion>{body}</project>"#)).unwrap();
        for classifier in ["", "-tests", "-client", "-sources", "-javadoc"] {
            fs::write(
                path.join(format!("{artifact}-{version}{classifier}.jar")),
                b"PK\x05\x06\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
            )
            .unwrap();
        }
    }
    pub fn request(&self, dependencies: serde_json::Value) -> DependencySpec {
        serde_json::from_value(json!({"repositories":[format!("file://{}",self.directory.path().display())],"classpaths":{"main":{"dependencies":dependencies}}})).unwrap()
    }
    pub fn resolve(&self, request: &DependencySpec) -> Result<ResolutionResult> {
        let mut repository = DirectoryRepository::new(vec![self.directory.path().into()]).unwrap();
        resolve(request, &mut repository)
    }
    pub fn ids(&self, request: &DependencySpec) -> Vec<String> {
        self.resolve(request).unwrap().classpaths["main"]
            .artifacts
            .iter()
            .map(|a| a.id.clone())
            .collect()
    }
}
pub fn dep(artifact: &str, version: &str, extra: &str) -> String {
    let version = if version.is_empty() {
        String::new()
    } else {
        format!("<version>{version}</version>")
    };
    format!(
        "<dependency><groupId>fixture</groupId><artifactId>{artifact}</artifactId>{version}{extra}</dependency>"
    )
}
pub fn deps(body: &str) -> String {
    format!("<dependencies>{body}</dependencies>")
}
pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
