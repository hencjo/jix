#!/usr/bin/env python3
"""Explicit refresh of published metadata; never run during ordinary locking/builds."""
import argparse
import hashlib
import http.server
import json
import pathlib
import subprocess
import tempfile
import threading
import urllib.error
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[1]
COORDINATES = [
    "com.google.guava:guava:33.5.0-jre",
    "org.eclipse.jetty:jetty-server:12.1.12",
    "org.eclipse.jetty.ee10:jetty-ee10-servlet:12.1.12",
    "org.glassfish.jersey.containers:jersey-container-servlet-core:3.1.11",
    "org.glassfish.jersey.inject:jersey-hk2:3.1.11",
    "org.apache.kafka:kafka-clients:4.1.2",
    "org.slf4j:slf4j-api:2.0.17",
    "org.slf4j:slf4j-simple:2.0.17",
    "org.yaml:snakeyaml:2.2",
    "com.squareup.okhttp3:okhttp:3.14.9",
    "jakarta.servlet:jakarta.servlet-api:6.0.0",
    "org.mockito:mockito-core:5.14.2",
]
CONTEXT = {"systemProperties": {
    "java.version": "21.0.12", "java.home": "/jix/declared-jdk-21", "os.name": "Linux", "os.arch": "amd64",
    "os.version": "6.6", "path.separator": ":",
}, "userProperties": {}}

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", default="https://repo.maven.apache.org/maven2")
    parser.add_argument("--binary", default=str(ROOT / "target/debug/jix"))
    args = parser.parse_args()
    if not args.repository.startswith("https://"):
        raise SystemExit("An explicit HTTPS repository is required")
    destination = ROOT / "fixtures/profile-published"
    destination.mkdir(exist_ok=True)
    previous_path = destination / "corpus.json"
    previous = json.loads(previous_path.read_text()) if previous_path.exists() else None
    if previous and previous["repository"] != args.repository:
        raise SystemExit("Refusing to relabel cached POMs from a different repository")
    records = {}
    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            relative = self.path.lstrip("/")
            if not relative.endswith(".pom") or ".." in relative.split("/"):
                self.send_error(404); return
            target = destination / "repository" / relative
            url = args.repository.rstrip("/") + "/" + relative
            try:
                if target.exists():
                    data = target.read_bytes()
                    if previous and relative in previous["poms"]:
                        expected = previous["poms"][relative]["sha256"]
                        if hashlib.sha256(data).hexdigest() != expected:
                            raise ValueError("Cached POM differs from its recorded SHA-256")
                else:
                    with urllib.request.urlopen(url, timeout=40) as response:
                        data = response.read(4 * 1024 * 1024 + 1)
                    if len(data) > 4 * 1024 * 1024:
                        raise ValueError("POM exceeds 4 MiB")
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.write_bytes(data)
                records[relative] = {"url": url, "sha256": hashlib.sha256(data).hexdigest()}
                self.send_response(200); self.end_headers(); self.wfile.write(data)
            except urllib.error.HTTPError as error:
                self.send_error(error.code)
            except Exception as error:
                self.send_error(502, str(error))
        def log_message(self, *args): pass
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    request = {"repositories": [f"http://127.0.0.1:{server.server_port}"],
               "resolutionContext": CONTEXT,
               "classpaths": {"compile": {"dependencies": COORDINATES},
                             "runtime": {"scope": "runtime", "dependencies": COORDINATES}}}
    try:
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "dependencies.json"
            path.write_text(json.dumps(request))
            result = subprocess.run([args.binary, "resolve", str(path)], capture_output=True, text=True)
            print(result.stderr, end="")
            if result.returncode:
                raise SystemExit(result.returncode)
            (destination / "selection.json").write_text(result.stdout)
            (destination / "corpus.json").write_text(json.dumps({"repository":args.repository, "coordinates":COORDINATES, "resolutionContext":CONTEXT, "poms":records}, indent=2, sort_keys=True) + "\n")
            print(f"Captured {len(records)} POMs")
    finally:
        server.shutdown()

if __name__ == "__main__":
    main()
