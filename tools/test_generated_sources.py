#!/usr/bin/env python3
"""Generated Java and pinned protobuf acceptance; IFD remains disabled."""
import json
import os
import pathlib
import shutil
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]


def run(*args, success=True, **kwargs):
    result = subprocess.run([str(a) for a in args], capture_output=True, text=True, timeout=900, **kwargs)
    assert (result.returncode == 0) == success, f"{args}\n{result.stdout}\n{result.stderr}"
    return result


def main():
    os.environ.pop("JIX_ENVIRONMENT", None)
    system = run("nix", "eval", "--impure", "--raw", "--expr", "builtins.currentSystem").stdout.strip()
    if system != "x86_64-linux":
        print("Skipped pinned protobuf example: only x86_64-linux has a verified compiler pin")
        return
    with tempfile.TemporaryDirectory(prefix="jix-generated-") as tmp:
        base = pathlib.Path(tmp)
        snapshot = base / "source"
        shutil.copytree(ROOT, snapshot, ignore=shutil.ignore_patterns(".aws", ".codex", ".agents", "dist", ".git", ".jix", ".devenv*", ".direnv", "target", "result", "__pycache__"))
        source = run("nix", "store", "add-path", snapshot).stdout.strip()
        binary = os.environ.get("JIX_TEST_BINARY")
        if not binary:
            package = run("nix", "build", f"path:{source}#jix", "--no-link", "--print-out-paths").stdout.strip()
            binary = str(pathlib.Path(package) / "bin/jix")
        binary = str(pathlib.Path(binary).resolve())
        project = base / "project"
        shutil.copytree(ROOT / "examples/protobuf", project)
        flake = (project / "flake.nix").read_text().replace("path:../..", f"path:{source}")
        (project / "flake.nix").write_text(flake)
        run("nix", "flake", "lock", f"path:{project}")
        before = (project / "jix.lock").read_bytes()
        run(binary, "verify", "--project", project)
        # New, unrealized output: evaluation must not inspect it or run protoc.
        recipe = (project / "build.nix").read_text().replace('"protobuf-java-sources"', f'"protobuf-java-sources-{base.name}"')
        recipe = recipe.replace('protoc --proto_path=', 'echo generation > "$out/marker"\n    protoc --proto_path=')
        (project / "build.nix").write_text(recipe)
        expression = f'''let f = builtins.getFlake "path:{project}"; in f.packages.x86_64-linux.jar.drvPath'''
        run("nix", "eval", "--impure", "--option", "allow-import-from-derivation", "false", "--raw", "--expr", expression)
        # Handwritten syntax errors must not force application compilation during IDE setup.
        main = project / "src/main/java/example/Main.java"
        valid = main.read_text()
        main.write_text("intentionally invalid Java")
        run(binary, "idea", "--project", project)
        iml = (project / ".idea/jix-modules/application.iml").read_text()
        assert 'generated="true"' in iml and "file:///nix/store/" in iml
        assert "file://$MODULE_DIR$/../../src/main/java" in iml, iml
        assert list((project / ".jix/roots").glob("idea-*protobuf-java-sources*"))
        run(binary, "build", "jar", "--project", project, success=False)
        main.write_text(valid)
        assert run(binary, "run", "--project", project).stdout == "Hello from protobuf\n"
        jar = run(binary, "build", "jar", "--project", project).stdout.strip()
        # Changes to schema regenerate/recompile.
        schema = project / "src/main/proto/message.proto"
        schema.write_text(schema.read_text().replace("string message = 1;", "string message = 1;\n  string extra = 2;"))
        jar2 = run(binary, "build", "jar", "--project", project).stdout.strip()
        assert jar != jar2
        run(binary, "idea", "--project", project)
        assert (project / ".jix/roots").is_dir()
        # Generated collisions and symlinks fail at build time, never through IFD.
        for command, expected in [
            ('mkdir -p "$out/java/example"; echo "class Main {}" > "$out/java/example/Main.java"', "Duplicate Java source"),
            ('ln -s missing "$out/java/Bad.java"', "Symlink source unsupported"),
            ('echo "module duplicate {}" > "$out/java/module-info.java"', "Duplicate Java source"),
        ]:
            bad_recipe = recipe.replace('mkdir -p "$out/java"', 'mkdir -p "$out/java"\n    ' + command)
            (project / "build.nix").write_text(bad_recipe)
            result = run(binary, "build", "jar", "--project", project, success=False)
            assert expected in result.stderr, result.stderr
        (project / "build.nix").write_text(recipe)
        # Same generated input works as an ordinary classpath build.
        (project / "src/main/java/module-info.java").unlink()
        plain = recipe.replace("modulepath", "classpath").replace('module = "example.protobuf/example.Main";', 'mainClass = "example.Main";')
        (project / "build.nix").write_text(plain)
        assert run(binary, "run", "--project", project).stdout == "Hello from protobuf\n"
        run(binary, "idea", "--project", project)
        # A directly targeted generated-only library is still an IDE module.
        only = plain.replace("./src/main/java", "")
        (project / "build.nix").write_text(only)
        bare_flake = '''{
          inputs.jix.url = "SOURCE";
          inputs.nixpkgs.follows = "jix/nixpkgs";
          outputs = {jix,nixpkgs,...}: let
            system = "x86_64-linux"; pkgs = import nixpkgs { inherit system; };
            deps = jix.dependencies { inherit pkgs; spec = import ./maven.nix; lockFile = ./jix.lock; };
            build = import ./build.nix { inherit pkgs deps; java = jix.lib { inherit pkgs; }; };
            project = jix.project { inherit pkgs deps; targets.jar = build.jar; defaultTarget = "jar"; };
          in { jix.${system}.dependencies = project.descriptor; packages.${system} = project.packages; };
        }'''.replace("SOURCE", f"path:{source}")
        (project / "flake.nix").write_text(bare_flake)
        run(binary, "idea", "--project", project)
        only_iml = (project / ".idea/jix-modules/application.iml").read_text()
        assert 'generated="true"' in only_iml and "/src/main/java" not in only_iml
        assert (project / "jix.lock").read_bytes() == before
    print("Generated-source integration passed: no IFD, protobuf, JPMS/classpath, IDE roots, generated-only modules and regeneration")


if __name__ == "__main__":
    main()
