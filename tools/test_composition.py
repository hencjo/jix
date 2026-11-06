#!/usr/bin/env python3
"""Host acceptance: monorepos, external library interfaces, and unmodified vendors."""
import hashlib
import json
import os
import pathlib
import shutil
import tempfile

from test_project import ROOT, run


def snapshot(path):
    return {str(p.relative_to(path)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in path.rglob("*") if p.is_file()}


def main():
    binary = os.environ.get("JIX_TEST_BINARY")
    if not binary:
        package = run("nix", "build", "path:.#jix", "--no-link", "--print-out-paths", cwd=ROOT).stdout.strip()
        binary = str(pathlib.Path(package) / "bin/jix")
    binary = str(pathlib.Path(binary).resolve())
    system = run("nix", "eval", "--impure", "--raw", "--expr", "builtins.currentSystem").stdout.strip()
    with tempfile.TemporaryDirectory(prefix="jix-composition-") as temp:
        base = pathlib.Path(temp)
        source = base / "source"
        shutil.copytree(ROOT, source, ignore=shutil.ignore_patterns(".aws", ".codex", ".agents", "dist",
            ".git", ".jix", ".devenv*", ".direnv", "target", "result", "__pycache__"))
        source = pathlib.Path(run("nix", "store", "add-path", source).stdout.strip())
        project = base / "project"
        shutil.copytree(ROOT / "examples/monorepo", project)
        flake = project / "flake.nix"
        flake.write_text(flake.read_text().replace("path:../..", f"path:{source}"))
        run("nix", "flake", "lock", f"path:{project}")
        vendor_before = snapshot(project / "vendor")
        independent_lock = (project / "build-support/parser/jix.lock").read_bytes()
        # Explicit locking migrates legacy fixtures once; subsequent locking is a no-op.
        run(binary, "lock", "--project", project)
        assert json.loads((project / "jix.lock").read_text())["version"] == 5
        root_lock = (project / "jix.lock").read_bytes()
        modified = (project / "jix.lock").stat().st_mtime_ns
        run(binary, "lock", "--project", project)
        assert (project / "jix.lock").stat().st_mtime_ns == modified

        def drv(name):
            return run("nix", "eval", "--raw", f"path:{project}#packages.{system}.{name}.drvPath").stdout.strip()

        def build(name):
            actual = run(binary, "build", name, "--project", project).stdout.strip()
            expected = run("nix", "build", f"path:{project}#{name}",
                           "--no-link", "--print-out-paths").stdout.strip()
            assert actual == expected
            return actual

        assert run(binary, "run", "--project", project).stdout == "composed\n"
        assert run("nix", "run", f"path:{project}").stdout == "composed\n"
        assert run(binary, "run", "plain", "--project", project).stdout == "composed\n"
        for target in ["parser", "core", "server", "unrelated", "dependencies", "plain"]:
            build(target)
        classes = run("nix", "eval", "--raw", f"path:{project}#packages.{system}.core.classes.drvPath").stdout
        before = {name: drv(name) for name in ["core", "server", "unrelated", "parser"]}
        resource = project / "libraries/core/resources/message.txt"
        original = resource.read_bytes()
        resource.write_bytes(original + b"resource edit\n")
        assert run("nix", "eval", "--raw", f"path:{project}#packages.{system}.core.classes.drvPath").stdout == classes
        assert drv("core") != before["core"] and drv("server") != before["server"]
        assert drv("unrelated") == before["unrelated"] and drv("parser") == before["parser"]
        resource.write_bytes(original)
        core_source = project / "libraries/core/src/core/Core.java"
        original = core_source.read_bytes()
        core_source.write_bytes(original + b"\n// edit\n")
        assert drv("core") != before["core"] and drv("server") != before["server"]
        assert drv("unrelated") == before["unrelated"]
        core_source.write_bytes(original)

        # External flake packages preserve their complete Java-library interface.
        consumer = base / "consumer"
        consumer.mkdir()
        (consumer / "flake.nix").write_text("""
        {
          inputs.upstream.url = "path:PROJECT";
          inputs.jix.follows = "upstream/jix";
          inputs.nixpkgs.follows = "upstream/nixpkgs";
          outputs = { upstream, jix, nixpkgs, ... }:
            let
              system = "SYSTEM";
              pkgs = import nixpkgs { inherit system; };
              java = jix.lib { inherit pkgs; };
              server = java.library {
                name = "external.server";
                sources = upstream + "/applications/server/src";
                moduleDependencies = [ upstream.packages.${system}.core ];
              };
            in { packages.${system}.default = java.application {
              name = "external-server";
              moduleDependencies = [ server ];
              module = "com.example.server/server.Main";
            }; };
        }
        """.replace("PROJECT", str(project)).replace("SYSTEM", system))
        assert run("nix", "run", f"path:{consumer}").stdout == "composed\n"

        # Omit dependency injection to select the adapter's independent pins.
        recipe = project / "build.nix"
        original_recipe = recipe.read_text()
        recipe.write_text(original_recipe.replace("deps = deps.components.parser;", ""))
        assert drv("parser") == before["parser"]
        run(binary, "run", "--project", project)
        (project / "build-support/parser/jix.lock").unlink()
        assert "lock" in run(binary, "build", "parser", "--project", project, success=False).stderr.lower()
        (project / "build-support/parser/jix.lock").write_bytes(independent_lock)
        recipe.write_text(original_recipe)

        # The compiler enforces JPMS exports; no silent classpath fallback.
        module = project / "libraries/core/src/module-info.java"
        original_module = module.read_text()
        module.write_text(original_module.replace("exports core;", ""))
        failed = run(binary, "build", "server", "--project", project, success=False)
        assert "does not export" in failed.stderr or "not visible" in failed.stderr, failed.stderr
        module.write_text(original_module)
        # Workspace declarations are independent of Java build evaluation.
        recipe.write_text('throw "build must not be evaluated while locking"')
        run(binary, "lock", "--project", project)
        recipe.write_text(original_recipe)
        assert snapshot(project / "vendor") == vendor_before
        assert (project / "build-support/parser/jix.lock").read_bytes() == independent_lock
        assert (project / "jix.lock").read_bytes() == root_lock

        # Relative source paths survive relocating the whole project.
        relocated = base / "relocated"
        project.rename(relocated)
        assert run(binary, "run", "--project", relocated).stdout == "composed\n"
        assert snapshot(relocated / "vendor") == vendor_before
        print("Composition passed: monorepo, JPMS, automatic modules, external flakes, vendor adapters, lock ownership, invalidation, relocation")


if __name__ == "__main__":
    main()
