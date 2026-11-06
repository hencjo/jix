#!/usr/bin/env python3
"""Host-side embedded backend acceptance tests; no Maven/Gradle build execution."""
import json
import functools
import http.server
import threading
import os
import pathlib
import shutil
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]

def run(*args, success=True, **kwargs):
    p = subprocess.run([str(a) for a in args], text=True, capture_output=True, timeout=900, **kwargs)
    if success and p.returncode:
        raise AssertionError(f"{args}\n{p.stderr}")
    if not success and p.returncode == 0:
        raise AssertionError(f"Unexpected success: {args}")
    return p

def main():
    binary = os.environ.get("JIX_TEST_BINARY")
    if not binary:
        package = run("nix", "build", "path:.#jix", "--no-link", "--print-out-paths", cwd=ROOT).stdout.strip()
        binary = str(pathlib.Path(package) / "bin/jix")
    binary = str(pathlib.Path(binary).resolve())
    with tempfile.TemporaryDirectory(prefix="jix-project-tests-") as temp:
        base = pathlib.Path(temp)
        source = base / "source"
        shutil.copytree(ROOT, source, ignore=shutil.ignore_patterns(".aws", ".codex", ".agents", "dist", ".git", ".jix", ".devenv*", ".direnv", "target", "result", "__pycache__"))
        source = pathlib.Path(run("nix", "store", "add-path", source).stdout.strip())
        fake_bin = base / "fake-bin"
        fake_bin.mkdir()
        fake_java = fake_bin / "java"
        fake_java.write_text("#!/bin/sh\n"
            "test -z \"$JAVA_TOOL_OPTIONS$JDK_JAVA_OPTIONS$_JAVA_OPTIONS\" || exit 42\n"
            "cat >&2 <<'PROPERTIES'\n"
            "    java.version = 19.0.2\n"
            "    java.home = /test/java-${literal}\n"
            "    os.name = Linux\n"
            "    os.arch = amd64\n"
            "    os.version = 6.6\n"
            "    path.separator = :\nPROPERTIES\n")
        fake_java.chmod(0o755)
        capture_env = dict(os.environ, PATH=f"{fake_bin}:{os.environ['PATH']}",
                           JAVA_TOOL_OPTIONS="-Djava.version=999")
        project = base / "project"
        run(binary, "init", "--project", project, "--jix-source", f"path:{source}", env=capture_env)
        context = (project / "resolution-context.nix").read_bytes()
        assert b'"java.version" = "19.0.2"' in context
        assert b'\\${literal}' in context
        fake_java.write_text("#!/bin/sh\nexit 99\n")
        # Existing init and frozen operations must not probe PATH Java again.
        run(binary, "init", "--project", project, "--jix-source", f"path:{source}", env=capture_env)
        assert (project / "resolution-context.nix").read_bytes() == context
        assert (project / "maven.nix").is_file()
        assert not (project / "dependencies.nix").exists()
        assert (project / "flake.lock").is_file()
        assert source in [p.resolve() for p in (project / ".jix/roots").glob("input-*")]
        assert not (project / "jix.lock").exists()
        flake = (project / "flake.nix").read_bytes()
        run(binary, "init", "--project", project)
        assert (project / "flake.nix").read_bytes() == flake
        failed = run(binary, "build", "--project", project, success=False)
        assert "run jix lock" in failed.stderr, failed.stderr
        run(binary, "lock", "--project", project)
        run(binary, "verify", "--project", project)
        frozen = (project / "jix.lock").read_bytes()
        assert b"19.0.2" in frozen
        run(binary, "lock", "--project", project, env=capture_env)
        run(binary, "verify", "--project", project, env=capture_env)
        assert (project / "jix.lock").read_bytes() == frozen
        assert (project / "resolution-context.nix").read_bytes() == context
        retry = base / "retry"
        run(binary, "init", "--project", retry, "--jix-source", f"path:{source}", env=capture_env, success=False)
        assert not (retry / "resolution-context.nix").exists()
        original_maven = (retry / "maven.nix").read_bytes()
        run(binary, "init", "--project", retry, "--jix-source", f"path:{source}")
        assert (retry / "resolution-context.nix").is_file()
        assert (retry / "maven.nix").read_bytes() == original_maven
        collision = base / "collision"
        collision.mkdir()
        (collision / "resolution-context.nix").write_text("keep me")
        run(binary, "init", "--project", collision, "--jix-source", f"path:{source}", success=False)
        assert (collision / "resolution-context.nix").read_text() == "keep me"
        assert not (collision / "flake.nix").exists()
        # Explicit legacy imports remain valid and do not change the lock.
        locked = (project / "jix.lock").read_bytes()
        (project / "maven.nix").rename(project / "dependencies.nix")
        (project / "flake.nix").write_bytes(flake.replace(b"./maven.nix", b"./dependencies.nix"))
        run(binary, "verify", "--project", project)
        run(binary, "lock", "--project", project)
        assert (project / "jix.lock").read_bytes() == locked
        (project / "dependencies.nix").rename(project / "maven.nix")
        (project / "flake.nix").write_bytes(flake)
        pins = (project / "flake.lock").read_bytes()
        # Use real local fixture bytes; nobody needs a remote Maven repository.
        repo = base / "repository"
        pom = repo / "t/a/1/a-1.pom"
        pom.parent.mkdir(parents=True)
        pom.write_text("<project><modelVersion>4.0.0</modelVersion><groupId>t</groupId><artifactId>a</artifactId><version>1</version></project>")
        pom.with_suffix(".jar").write_bytes(b"test artifact")
        class Handler(http.server.SimpleHTTPRequestHandler):
            def do_GET(self):
                self.server.requests.append(self.path)
                super().do_GET()
        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(Handler, directory=repo))
        server.requests = []
        threading.Thread(target=server.serve_forever, daemon=True).start()
        repository_url = f"http://127.0.0.1:{server.server_port}"
        spec = project / "maven.nix"
        spec.write_text('{ repositories = [ ' + json.dumps(repository_url) + ' ]; classpaths.main.dependencies = [ "t:a:1" ]; }')
        failed = run(binary, "verify", "--project", project, success=False)
        assert "Stale lockfile" in failed.stderr, failed.stderr
        run(binary, "lock", "--project", project)
        assert (project / "flake.lock").read_bytes() == pins
        realized = run(binary, "build", "--project", project).stdout.strip()
        assert pathlib.Path(realized, "artifacts.json").is_file()
        assert (project / ".jix/roots/all").resolve() == pathlib.Path(realized)
        direct = run("nix", "build", f"path:{project}#dependencies", "--no-link", "--print-out-paths").stdout.strip()
        assert direct == realized
        assert run(binary, "build", "dependencies", "--project", project).stdout.strip() == realized
        run(binary, "build", "missing", "--project", project, success=False)
        run(binary, "info", "--project", project, "--json")
        cp = run(binary, "classpath", "main", "--project", project).stdout.strip()
        assert pathlib.Path(cp).read_bytes() == b"test artifact"
        assert (project / ".jix/roots/classpath-main").is_symlink()
        roots = run("nix-store", "--query", "--roots", realized).stdout
        assert str(project / ".jix/roots/all") in roots
        export = base / "export"
        run(binary, "export", "--project", project, "--output", export)
        moved = base / "moved"
        export.rename(moved)
        run(binary, "verify", moved / "jix.lock", "--artifacts", moved / "artifacts.json")
        assert not any(p.is_symlink() for p in moved.rglob("*"))
        run(binary, "internal", "classpath", moved / "classpaths/main/classpath.json", "--output", base / "cp")
        (moved / "repository/t/a/1/a-1.jar").write_bytes(b"corrupt")
        assert "SHA-256 mismatch" in run(binary, "verify", moved / "jix.lock", "--artifacts", moved / "artifacts.json", success=False).stderr
        # Offline verification must not retrieve uncached network expressions.
        spec.write_text('{ repositories = [ (builtins.fetchurl ' + json.dumps(repository_url + "/not-cached") + ') ]; classpaths.main.dependencies = []; }')
        before = len(server.requests)
        run(binary, "verify", "--project", project, success=False)
        assert len(server.requests) == before, "offline verification made an HTTP request"
        assert (project / ".jix/roots/all").resolve() == pathlib.Path(realized)
        old_lock = (project / "jix.lock").read_bytes()
        run(binary, "lock", "--project", project, success=False)
        assert (project / "jix.lock").read_bytes() == old_lock
        # Runtime state and build artifacts cannot alter project evaluation.
        (project / "target").mkdir()
        (project / "target/ignored").write_text("not a source input")
        app = base / "app"
        run(binary, "init", "--project", app, "--template", "java", "--jix-source", f"path:{source}", env=capture_env)
        app_context = (app / "resolution-context.nix").read_text()
        assert '"java.version" = "21.' in app_context
        assert '"java.home" = "/nix/store/' in app_context
        run(binary, "lock", "--project", app)
        frozen = (app / "jix.lock").read_bytes()
        build_file = app / "build.nix"
        original_build = build_file.read_text()
        build_file.write_text('throw "Java DSL must not be evaluated"')
        run(binary, "build", "dependencies", "--project", app)
        build_file.write_text(original_build)
        application = run(binary, "build", "--project", app).stdout.strip()
        assert (app / ".jix/roots/target-application").resolve() == pathlib.Path(application)
        assert run(pathlib.Path(application) / "bin/application").stdout == "Hello from Jix!\n"
        assert run("nix", "run", f"path:{app}").stdout == "Hello from Jix!\n"
        assert run(binary, "run", "--project", app).stdout == "Hello from Jix!\n"
        assert run(binary, "run", "application", "--project", app).stdout == "Hello from Jix!\n"
        assert list((app / ".jix/roots").glob("app-default-*"))
        run(binary, "run", "missing", "--project", app, success=False)
        for target, package in [("application", "application"), ("jar", "jar"),
                                ("classpaths.main", "classpath-main")]:
            actual = run(binary, "build", target, "--project", app).stdout.strip()
            expected = run("nix", "build", f"path:{app}#{package}", "--no-link", "--print-out-paths").stdout.strip()
            assert actual == expected
        assert run("nix", "build", f"path:{app}", "--no-link", "--print-out-paths").stdout.strip() == application
        consumer = base / "consumer"
        consumer.mkdir()
        (consumer / "flake.nix").write_text(
            '{ inputs.upstream.url = "path:' + str(app) + '"; outputs = { upstream, ... }: { packages = upstream.packages; }; }')
        assert run("nix", "build", f"path:{consumer}#jar", "--no-link", "--print-out-paths").stdout.strip() == run(binary, "build", "jar", "--project", app).stdout.strip()
        build_file.write_text("""{ pkgs, ... }: {
          app = pkgs.writeShellScriptBin "application" ''
            case "$1" in signal) kill -TERM $$ ;; esac
            printf '%s\\n' "$PWD" "$JIX_RUN_TEST" "$@"
            exit 23
          '';
        }""")
        env = dict(os.environ, JIX_RUN_TEST="inherited")
        forwarded = ["--help", "with spaces", "$(not-a-shell)", ""]
        launched = run(binary, "run", "--project", app, "--", *forwarded, success=False, cwd=base, env=env)
        assert launched.returncode == 23
        assert launched.stdout.splitlines() == [str(base), "inherited", *forwarded]
        direct_run = run("nix", "run", f"path:{app}", "--", *forwarded, success=False, cwd=base, env=env)
        assert (direct_run.returncode, direct_run.stdout) == (launched.returncode, launched.stdout)
        raw = subprocess.run([os.fsencode(binary), b"run", b"--project", os.fsencode(app), b"--", b"\xff"],
                             cwd=base, env=env, capture_output=True, timeout=900)
        assert raw.returncode == 23 and raw.stdout.endswith(b"\xff\n")
        import signal
        assert run(binary, "run", "--project", app, "--", "signal", success=False).returncode == -signal.SIGTERM
        build_file.write_text(original_build)
        declarations = app / "maven.nix"
        original_declarations = declarations.read_text()
        declarations.write_text('{ classpaths.changed.dependencies = []; }')
        assert "lock" in run(binary, "run", "--project", app, success=False).stderr.lower()
        run("nix", "eval", "--raw", f"path:{app}#apps.x86_64-linux.default.program", success=False)
        declarations.write_text(original_declarations)
        assert (app / "jix.lock").read_bytes() == frozen
        # A v1 descriptor still supports default dependency and classpath builds.
        legacy_file = project / "flake.nix"
        legacy_file.write_text(legacy_file.read_text().replace(
            "projects.${system}.descriptor;", "projects.${system}.descriptor // { version = 1; };"))
        spec.write_text('{ repositories = [ ' + json.dumps(repository_url) + ' ]; classpaths.main.dependencies = [ "t:a:1" ]; }')
        assert run(binary, "build", "--project", project).stdout.strip() == realized
        assert run(binary, "classpath", "main", "--project", project).stdout.strip() == cp
        server.shutdown()
        print("Embedded project integration passed: init, lock, offline verification, realization, roots, direct Nix parity, export, Java template")

if __name__ == "__main__":
    main()
