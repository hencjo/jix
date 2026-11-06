use crate::{Error, ErrorKind, Result};
use std::path::{Component, Path, PathBuf};

/// Read a Maven repository-relative path from explicitly configured repositories.
/// Returning `None` means absent, not an I/O or authentication failure.
pub trait RepositoryProvider {
    fn read(&mut self, path: &str) -> Result<Option<Vec<u8>>>;
}

/// Ordered, read-only fixture repositories. No home directory or Maven cache lookup.
pub struct DirectoryRepository {
    roots: Vec<PathBuf>,
    origins: std::collections::BTreeMap<String, String>,
}
impl DirectoryRepository {
    pub fn origins(&self) -> &std::collections::BTreeMap<String, String> {
        &self.origins
    }
    pub fn new(roots: Vec<PathBuf>) -> Result<Self> {
        if roots.is_empty() {
            return Err(Error::invalid("At least one repository is required"));
        }
        let roots = roots
            .into_iter()
            .map(|p| {
                p.canonicalize()
                    .map_err(|e| Error::new(ErrorKind::Io, e.to_string()))
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            roots,
            origins: Default::default(),
        })
    }
}
impl RepositoryProvider for DirectoryRepository {
    fn read(&mut self, path: &str) -> Result<Option<Vec<u8>>> {
        if Path::new(path)
            .components()
            .any(|p| !matches!(p, Component::Normal(_)))
        {
            return Err(Error::invalid(
                "Repository path must be relative and traversal-free",
            ));
        }
        for root in &self.roots {
            let file = match root.join(path).canonicalize() {
                Ok(p) => p,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(Error::new(ErrorKind::Io, e.to_string())),
            };
            if !file.starts_with(root) {
                return Err(Error::invalid("Repository symlink escapes its root"));
            }
            let bytes =
                std::fs::read(file).map_err(|e| Error::new(ErrorKind::Io, e.to_string()))?;
            self.origins
                .insert(path.into(), format!("file://{}", root.display()));
            return Ok(Some(bytes));
        }
        Ok(None)
    }
}
