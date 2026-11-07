use jix_resolver::{Error, ErrorKind, RepositoryProvider, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Read, path::Path, process::Command};

const MAX_FILE: u64 = 256 * 1024 * 1024;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactSource {
    pub repository: String,
    pub url: String,
    pub sha256: String,
}
pub fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn io(e: impl std::fmt::Display) -> Error {
    Error::new(ErrorKind::Io, e.to_string())
}

/// Ordered, per-invocation repository snapshot. No user Maven/curl configuration.
pub struct Repository {
    urls: Vec<String>,
    cache: BTreeMap<String, Option<Vec<u8>>>,
    pub sources: BTreeMap<String, ArtifactSource>,
}
impl Repository {
    pub fn new(urls: &[String]) -> Result<Self> {
        if urls.is_empty() {
            return Err(Error::new(
                ErrorKind::Invalid,
                "At least one repository is required",
            ));
        }
        for url in urls {
            validate_url(url)?;
        }
        Ok(Self {
            urls: urls.to_vec(),
            cache: BTreeMap::new(),
            sources: BTreeMap::new(),
        })
    }
    fn fetch(repository: &str, path: &str) -> Result<Option<Vec<u8>>> {
        if let Some(root) = repository.strip_prefix("file://") {
            let root = Path::new(root).canonicalize().map_err(io)?;
            let file = match root.join(path).canonicalize() {
                Ok(path) => path,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(io(e)),
            };
            if !file.starts_with(&root) {
                return Err(Error::new(
                    ErrorKind::Invalid,
                    "Repository symlink escapes root",
                ));
            }
            let mut bytes = Vec::new();
            std::fs::File::open(file)
                .map_err(io)?
                .take(MAX_FILE + 1)
                .read_to_end(&mut bytes)
                .map_err(io)?;
            if bytes.len() as u64 > MAX_FILE {
                return Err(Error::new(ErrorKind::Limit, "Artifact exceeds 256 MiB"));
            }
            return Ok(Some(bytes));
        }
        let file = tempfile::NamedTempFile::new().map_err(io)?;
        let url = format!("{}/{path}", repository.trim_end_matches('/'));
        let output = Command::new("curl")
            .args([
                "--disable",
                "--silent",
                "--show-error",
                "--location",
                "--fail",
                "--proto",
                "=https,http",
                "--proto-redir",
                "=https",
                "--connect-timeout",
                "10",
                "--max-time",
                "120",
                // curl retries timeouts and transient HTTP errors with exponential
                // backoff, resetting the output file before each attempt. Do not
                // retry all errors: 404 must still permit repository fallback.
                "--retry",
                "3",
                "--retry-max-time",
                "300",
                "--retry-connrefused",
                "--max-filesize",
                "268435456",
                "--write-out",
                "%{http_code}",
                "--output",
            ])
            .arg(file.path())
            .arg(&url)
            .output()
            .map_err(|e| io(format!("Cannot run curl: {e}")))?;
        let status = String::from_utf8_lossy(&output.stdout);
        if output.status.code() == Some(22) && status.trim() == "404" {
            return Ok(None);
        }
        if !output.status.success() {
            return Err(io(format!(
                "Fetch {url} failed (HTTP {}): {}",
                status.trim(),
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(Some(std::fs::read(file.path()).map_err(io)?))
    }
}
impl RepositoryProvider for Repository {
    fn read(&mut self, path: &str) -> Result<Option<Vec<u8>>> {
        if path.is_empty()
            || path.split('/').any(|s| {
                s.is_empty()
                    || s == "."
                    || s == ".."
                    || !s
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            })
        {
            return Err(Error::new(
                ErrorKind::Invalid,
                "Unsafe repository-relative path",
            ));
        }
        if let Some(bytes) = self.cache.get(path) {
            return Ok(bytes.clone());
        }
        for repository in &self.urls {
            if let Some(bytes) = Self::fetch(repository, path)? {
                let source = ArtifactSource {
                    repository: repository.clone(),
                    url: format!("{}/{path}", repository.trim_end_matches('/')),
                    sha256: sha256(&bytes),
                };
                self.sources.insert(path.into(), source);
                self.cache.insert(path.into(), Some(bytes.clone()));
                return Ok(Some(bytes));
            }
        }
        self.cache.insert(path.into(), None);
        Ok(None)
    }
}
pub fn validate_url(url: &str) -> Result<()> {
    let bad = || {
        Error::new(
            ErrorKind::Invalid,
            format!(
                "Invalid repository URL: {url:?}. Use HTTPS or an absolute file:// URL; HTTP is allowed only on loopback."
            ),
        )
    };
    if !url.is_ascii()
        || url.bytes().any(|b| b.is_ascii_whitespace())
        || url.contains(['?', '#', '%', '\\'])
    {
        return Err(bad());
    }
    if let Some(path) = url.strip_prefix("file://") {
        if !path.starts_with('/') || path.split('/').any(|s| matches!(s, "." | "..")) {
            return Err(bad());
        }
        return Ok(());
    }
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or_else(bad)?;
    let host = rest.split('/').next().unwrap_or("");
    if host.is_empty()
        || host.contains('@')
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-:".contains(&b))
    {
        return Err(bad());
    }
    if url.starts_with("http://")
        && !matches!(host.split(':').next(), Some("127.0.0.1" | "localhost"))
    {
        return Err(bad());
    }
    if rest.split('/').any(|s| matches!(s, "." | "..")) {
        return Err(bad());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        thread,
        time::Duration,
    };

    // Exercise the real curl invocation without Internet access or PATH mocks.
    fn with_server(responses: &[(u16, &str)], check: impl FnOnce(&str)) -> usize {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let responses: Vec<_> = responses.iter().map(|(s, b)| (*s, b.to_string())).collect();
        let stop = Arc::new(AtomicBool::new(false));
        let server_stop = stop.clone();
        let server = thread::spawn(move || {
            let mut count = 0;
            while !server_stop.load(Ordering::Relaxed) {
                let (mut stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(e) => panic!("accept: {e}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = String::new();
                let mut reader = BufReader::new(&stream);
                while !request.ends_with("\r\n\r\n") {
                    assert!(reader.read_line(&mut request).unwrap() > 0);
                }
                let (status, body) = responses.get(count).expect("unexpected retry");
                count += 1;
                write!(stream,
                    "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                ).unwrap();
            }
            count
        });
        check(&url);
        stop.store(true, Ordering::Relaxed);
        server.join().unwrap()
    }

    #[test]
    fn transient_downloads_retry_and_cache_only_success() {
        let count = with_server(
            &[(408, "timeout"), (503, "unavailable"), (200, "artifact")],
            |url| {
                let mut repo = Repository::new(&[url.into()]).unwrap();
                assert_eq!(repo.read("a.jar").unwrap().unwrap(), b"artifact");
                assert_eq!(repo.sources["a.jar"].sha256, sha256(b"artifact"));
                assert_eq!(repo.read("a.jar").unwrap().unwrap(), b"artifact");
            },
        );
        assert_eq!(count, 3);
    }

    #[test]
    fn retry_exhaustion_is_an_error_and_does_not_poison_cache() {
        let count = with_server(
            &[
                (503, "busy"),
                (503, "busy"),
                (503, "busy"),
                (503, "busy"),
                (200, "ok"),
            ],
            |url| {
                let mut repo = Repository::new(&[url.into()]).unwrap();
                assert!(repo.read("a.jar").unwrap_err().to_string().contains("503"));
                assert!(repo.sources.is_empty());
                assert_eq!(repo.read("a.jar").unwrap().unwrap(), b"ok");
            },
        );
        assert_eq!(count, 5);
    }

    #[test]
    fn missing_artifacts_fall_back_without_retries() {
        let first_count = with_server(&[(404, "missing")], |first| {
            let second_count = with_server(&[(200, "found")], |second| {
                let mut repo = Repository::new(&[first.into(), second.into()]).unwrap();
                assert_eq!(repo.read("a.jar").unwrap().unwrap(), b"found");
                assert_eq!(repo.sources["a.jar"].repository, second);
            });
            assert_eq!(second_count, 1);
        });
        assert_eq!(first_count, 1);
    }

    #[test]
    fn permanent_http_errors_do_not_retry_or_fall_back() {
        let count = with_server(&[(403, "forbidden")], |url| {
            let mut repo = Repository::new(&[url.into(), "http://127.0.0.1:1".into()]).unwrap();
            assert!(repo.read("a.jar").unwrap_err().to_string().contains("403"));
            assert!(repo.sources.is_empty());
        });
        assert_eq!(count, 1);
    }
}
