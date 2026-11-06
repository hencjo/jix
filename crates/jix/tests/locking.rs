use jix::{
    lock,
    repository::{Repository, validate_url},
};
use jix_resolver::{DependencySpec, RepositoryProvider};
use serde_json::json;
use std::fs;
fn fixture() -> (tempfile::TempDir, DependencySpec) {
    let dir = tempfile::tempdir().unwrap();
    let artifact = dir.path().join("t/a/1");
    fs::create_dir_all(&artifact).unwrap();
    fs::write(artifact.join("a-1.pom"),"<project><modelVersion>4.0.0</modelVersion><groupId>t</groupId><artifactId>a</artifactId><version>1</version></project>").unwrap();
    fs::write(artifact.join("a-1.jar"), b"fixture-binary").unwrap();
    let request=serde_json::from_value(json!({"repositories":[format!("file://{}",dir.path().display())],"classpaths":{"a":{"dependencies":["t:a:1"]},"b":{"dependencies":["t:a:1"]}}})).unwrap();
    (dir, request)
}
#[test]
fn deterministic_shared_and_pinned() {
    let (_dir, request) = fixture();
    let a = lock::create(&request).unwrap();
    let b = lock::create(&request).unwrap();
    assert_eq!(
        serde_json::to_value(&a).unwrap(),
        serde_json::to_value(&b).unwrap()
    );
    assert_eq!(a.artifacts.len(), 1);
    assert_eq!(a.provenance.len(), 2);
    assert_eq!(a.context_models.values().map(|m| m.len()).sum::<usize>(), 1);
    assert_eq!(
        a.artifacts["t:a:jar::1"].source.sha256,
        jix::repository::sha256(b"fixture-binary")
    );
    lock::validate(&a, &request).unwrap();
}
#[test]
fn stale_and_invalid_locks_fail_offline() {
    let (dir, mut request) = fixture();
    let original = lock::create(&request).unwrap();
    fs::remove_dir_all(dir.path().join("t")).unwrap();
    lock::validate(&original, &request).unwrap();
    request.classpaths.remove("b");
    assert!(lock::validate(&original, &request).is_err());
    let mut bad = original.clone();
    bad.version = 99;
    assert!(lock::validate(&bad, &original.spec).is_err());
    bad = original.clone();
    bad.artifacts.clear();
    assert!(lock::validate(&bad, &original.spec).is_err());
    bad = original.clone();
    bad.artifacts.get_mut("t:a:jar::1").unwrap().source.sha256 = "bad".into();
    assert!(lock::validate(&bad, &original.spec).is_err());
}
#[test]
fn writes_atomically_and_does_not_overwrite_on_failure() {
    let (dir, request) = fixture();
    let path = dir.path().join("jix.lock");
    let a = lock::create(&request).unwrap();
    lock::write(&a, &path).unwrap();
    let before = fs::read(&path).unwrap();
    fs::remove_file(dir.path().join("t/a/1/a-1.jar")).unwrap();
    assert!(lock::create(&request).is_err());
    assert_eq!(before, fs::read(&path).unwrap());
    lock::validate(&lock::read(&path).unwrap(), &request).unwrap();
}
#[test]
fn repository_failure_is_not_fallback() {
    let (dir, request) = fixture();
    let mut repo = Repository::new(&request.repositories).unwrap();
    assert!(repo.read("../a").is_err());
    let bytes = repo.read("t/a/1/a-1.jar").unwrap();
    fs::write(dir.path().join("t/a/1/a-1.jar"), b"changed").unwrap();
    assert_eq!(bytes, repo.read("t/a/1/a-1.jar").unwrap());
    for url in [
        "https://user:password@example.com/maven",
        "https://example.com/../secret",
        "http://example.com",
        "file://relative",
        "https://example.com/?token=x",
    ] {
        assert!(validate_url(url).is_err(), "{url}");
    }
}
#[test]
fn canonical_hash_matches_expanded_defaults() {
    let (_, a) = fixture();
    let json = serde_json::to_value(&a).unwrap();
    let b: DependencySpec = serde_json::from_value(json).unwrap();
    assert_eq!(lock::spec_hash(&a).unwrap(), lock::spec_hash(&b).unwrap());
}

#[test]
fn http_status_and_repository_precedence() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };
    for status in [200, 404, 401, 500] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(value) => break value,
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            && std::time::Instant::now() < deadline =>
                    {
                        thread::sleep(std::time::Duration::from_millis(10))
                    }
                    Err(e) => panic!("HTTP fixture did not receive a request: {e}"),
                }
            };
            let mut request = [0u8; 4096];
            let _ = stream.read(&mut request).unwrap();
            write!(
                stream,
                "HTTP/1.1 {status} Test\r\nContent-Length: 6\r\nConnection: close\r\n\r\nremote"
            )
            .unwrap();
        });
        let (_dir, request) = fixture();
        let mut repository =
            Repository::new(&[url.clone(), request.repositories[0].clone()]).unwrap();
        let result = repository.read("t/a/1/a-1.jar");
        server.join().unwrap();
        match status {
            200 => {
                assert_eq!(result.unwrap().unwrap(), b"remote");
                assert_eq!(repository.sources["t/a/1/a-1.jar"].repository, url);
            }
            404 => assert_eq!(result.unwrap().unwrap(), b"fixture-binary"),
            _ => assert!(result.is_err(), "HTTP {status} must not silently fall back"),
        }
    }
}
