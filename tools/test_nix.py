#!/usr/bin/env python3
"""Host-side Nix integration: invalidation, stale locks, tampering, and substitution."""
import hashlib
import json
import pathlib
import shutil
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]


def run(*args, success=True):
    result = subprocess.run(args, text=True, capture_output=True, timeout=300)
    if success and result.returncode:
        raise RuntimeError(f"{' '.join(args)}\n{result.stderr}")
    if not success and not result.returncode:
        raise AssertionError(f"Unexpected success: {' '.join(args)}")
    return result


def main():
    system = run("nix", "eval", "--impure", "--raw", "--expr", "builtins.currentSystem").stdout
    with tempfile.TemporaryDirectory(prefix="jix-integration-") as temp:
        base = pathlib.Path(temp)
        project = base / "project"
        shutil.copytree(ROOT, project, ignore=shutil.ignore_patterns(".aws", ".codex", ".agents", "dist", ".git", ".devenv*", ".direnv", "target", "__pycache__", ".jix", "result"))
        flake = f"path:{project}"
        def drv(name):
            return run("nix", "eval", "--raw", f"{flake}#packages.{system}.{name}.drvPath").stdout
        classes, archive, main_cp, runtime_cp = map(drv, ["hello-classes", "hello-jar", "classpath-main", "classpath-runtime"])
        resource = project / "examples/hello/src/main/resources/message.txt"
        original_resource = resource.read_bytes()
        resource.write_text("resource edit\n")
        assert drv("hello-classes") == classes, "resource edits invalidated javac"
        assert drv("hello-jar") != archive, "resource edits failed to invalidate the JAR"
        resource.write_bytes(original_resource)
        source = project / "examples/hello/src/main/java/example/Main.java"
        original_source = source.read_bytes()
        source.write_bytes(original_source + b"\n// source edit\n")
        assert drv("hello-classes") != classes, "source edits did not invalidate javac"
        source.write_bytes(original_source)

        request_path = project / "examples/hello/maven.nix"
        lock_path = project / "examples/hello/jix.lock"
        original_request, original_lock = request_path.read_bytes(), lock_path.read_bytes()
        request_path.write_text('dependencies = [];'.join(original_request.decode().rsplit('dependencies = common;', 1)))
        failed = run("nix", "eval", "--raw", f"{flake}#packages.{system}.classpath-main.drvPath", success=False)
        assert "stale lockfile" in failed.stderr
        lock = json.loads(original_lock)
        lock["request"]["classpaths"]["runtime"]["dependencies"] = []
        lock["requestHash"] = hashlib.sha256(json.dumps(lock["request"], sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()).hexdigest()
        lock["resolution"]["classpaths"]["runtime"] = {"dependencies": [], "artifacts": [], "edges": []}
        lock_path.write_text(json.dumps(lock))
        assert drv("classpath-main") == main_cp, "unrelated classpath edit invalidated main"
        assert drv("classpath-runtime") != runtime_cp
        request_path.write_bytes(original_request)
        lock_path.write_bytes(original_lock)

        # A genuine artifact hash mismatch must fail fetching, not resolve afresh.
        lock = json.loads(original_lock)
        path = next(path for path in lock["provenance"] if path.endswith(".jar"))
        lock["provenance"][path]["sha256"] = "0" * 64
        lock_path.write_text(json.dumps(lock))
        failed = run("nix", "build", f"{flake}#classpath-main", "--no-link", "--option", "substituters", "", success=False)
        assert "hash mismatch" in failed.stderr, failed.stderr
        lock_path.write_bytes(original_lock)

        jar = pathlib.Path(json.loads(run("nix", "build", f"{flake}#hello-jar", "--no-link", "--json").stdout)[0]["outputs"]["out"])
        cache = base / "cache"
        store = base / "isolated-store"
        run("nix", "copy", "--to", cache.as_uri(), str(jar))
        run("nix", "copy", "--from", cache.as_uri(), "--to", f"local?root={store}", "--no-check-sigs", str(jar))
        substituted = store / str(jar).lstrip("/")
        assert substituted.read_bytes() == jar.read_bytes(), "cache substitution changed JAR bytes"
        print("Nix integration passed: invalidation, independent classpaths, stale locks, hash rejection, isolated cache substitution")


if __name__ == "__main__":
    main()
