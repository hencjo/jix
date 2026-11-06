#!/usr/bin/env python3
"""Rust/Nix context normalization, lock ownership and legacy format parity."""
import hashlib
import json
import os
import pathlib
import shutil
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
BINARY = os.environ.get("JIX_TEST_BINARY")


def run(*args, success=True):
    p = subprocess.run([str(a) for a in args], capture_output=True, text=True, timeout=180)
    assert (p.returncode == 0) == success, p.stderr
    return p.stdout


def main():
    with tempfile.TemporaryDirectory(prefix="jix-context-") as tmp:
        base = pathlib.Path(tmp)
        source = base / "source"
        shutil.copytree(ROOT, source, ignore=shutil.ignore_patterns(".aws", ".codex", ".agents", "dist", ".git", ".devenv*", ".direnv", "target", "__pycache__", ".jix", "result"))
        binary = BINARY
        if not binary:
            package = run("nix", "build", "path:" + str(source) + "#jix", "--no-link", "--print-out-paths").strip()
            binary = pathlib.Path(package) / "bin/jix"
        repo = base / "repo/t/a/1"
        repo.mkdir(parents=True)
        (repo / "a-1.pom").write_text("<project><modelVersion>4.0.0</modelVersion><groupId>t</groupId><artifactId>a</artifactId><version>1</version></project>")
        (repo / "a-1.jar").write_bytes(b"fixture")
        spec = {
            "repositories": [f"file://{base / 'repo'}"],
            "resolutionContext": {"systemProperties": {"java.version": "21"}, "userProperties": {"flag": "yes"}},
            "classpaths": {
                "main": {"dependencies": ["t:a:1"]},
                "other": {"dependencies": ["t:a:1"], "resolutionContext": {"systemProperties": {"java.version": "17"}}},
            },
        }
        request = base / "dependencies.json"
        lock_path = base / "jix.lock"
        request.write_text(json.dumps(spec))
        run(binary, "lock", request, "--output", lock_path)
        lock = json.loads(lock_path.read_text())
        prefix = f"""
          let
            flake = builtins.getFlake {json.dumps('path:' + str(source))};
            pkgs = import flake.inputs.nixpkgs {{ system = builtins.currentSystem; }};
            api = import {source / 'nix/dependencies.nix'} {{ inherit pkgs; jix = "/unused"; }};
            spec = builtins.fromJSON (builtins.readFile {request});
          in """
        def evaluate(expr, success=True):
            return run("nix", "eval", "--impure", "--json", "--expr", prefix + expr, success=success)
        normalized = json.loads(evaluate("(api.dependencySpec spec).spec"))
        assert normalized == lock["request"]
        assert normalized["classpaths"]["other"]["resolutionContext"]["userProperties"] == {}
        digest = hashlib.sha256(json.dumps(normalized, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
        assert digest == lock["requestHash"]
        expression = f"(api.dependencies {{ inherit spec; lockFile = {lock_path}; }}).all.drvPath"
        evaluate(expression)
        for mutate in [
            lambda v: v["classpathInputs"]["other"].update(contextHash="0" * 64),
            lambda v: v["contextModels"].update(unowned={}),
            lambda v: next(iter(v["provenance"].values())).update(repository=-1),
            lambda v: next(iter(v["provenance"].values())).update(repository=99),
            lambda v: next(iter(v["provenance"].values())).update(unknown=True),
            lambda v: v.update(kind="matrix"),
            lambda v: v.update(unknown=True),
        ]:
            broken = json.loads(json.dumps(lock))
            mutate(broken)
            lock_path.write_text(json.dumps(broken))
            evaluate(expression, success=False)
            run(binary, "verify", lock_path, success=False)
        # Every old version is rejected by both consumers, with upgrade guidance.
        for version in [1, 2, 3, 4]:
            legacy = json.loads(json.dumps(lock))
            legacy["version"] = version
            lock_path.write_text(json.dumps(legacy))
            evaluate(expression, success=False)
            run(binary, "verify", lock_path, success=False)
        # Validate a malformed shared context even with no classpaths to inherit it.
        request.write_text(json.dumps({"repositories": [], "classpaths": {}, "resolutionContext": {"systemProperties": {"bad": 1}}}))
        evaluate("(api.dependencySpec spec).spec", success=False)
    print("resolution context Rust/Nix parity passed")


if __name__ == "__main__":
    main()
