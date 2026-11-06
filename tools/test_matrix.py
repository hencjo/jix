#!/usr/bin/env python3
"""Host acceptance for frozen environments, defaults and explicit overrides."""
import functools
import http.server
import json
import os
import pathlib
import shutil
import subprocess
import tempfile
import threading
import zipfile

ROOT = pathlib.Path(__file__).resolve().parents[1]


def run(*args, success=True, **kwargs):
    p = subprocess.run([str(a) for a in args], text=True, capture_output=True, timeout=900, **kwargs)
    assert (p.returncode == 0) == success, f"{args}\n{p.stdout}\n{p.stderr}"
    return p


def main():
    os.environ.pop("JIX_ENVIRONMENT", None)
    with tempfile.TemporaryDirectory(prefix="jix-matrix-") as temp:
        base = pathlib.Path(temp)
        source = base / "source"
        shutil.copytree(ROOT, source, ignore=shutil.ignore_patterns(".aws", ".codex", ".agents", "dist", ".git", ".devenv*", ".direnv", "target", "__pycache__", ".jix", "result"))
        source = pathlib.Path(run("nix", "store", "add-path", source).stdout.strip())
        binary = os.environ.get("JIX_TEST_BINARY")
        if not binary:
            package = run("nix", "build", f"path:{source}#jix", "--no-link", "--print-out-paths").stdout.strip()
            binary = str(pathlib.Path(package) / "bin/jix")
        binary = str(pathlib.Path(binary).resolve())
        project = base / "project"
        run(binary, "init", "--project", project, "--jix-source", f"path:{source}")
        repo = base / "repo"
        for name in ["a", "x", "y"]:
            path = repo / f"t/{name}/1/{name}-1.pom"
            path.parent.mkdir(parents=True)
            profiles = "" if name != "a" else """<profiles><profile><id>default</id><activation><activeByDefault>true</activeByDefault></activation><dependencies><dependency><groupId>t</groupId><artifactId>x</artifactId><version>1</version></dependency></dependencies></profile><profile><id>arm</id><activation><os><arch>aarch64</arch></os></activation><dependencies><dependency><groupId>t</groupId><artifactId>y</artifactId><version>1</version></dependency></dependencies></profile></profiles>"""
            path.write_text(f"<project><modelVersion>4.0.0</modelVersion><groupId>t</groupId><artifactId>{name}</artifactId><version>1</version>{profiles}</project>")
            with zipfile.ZipFile(path.with_suffix(".jar"), "w") as jar:
                jar.writestr(name + ".txt", name)
        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(http.server.SimpleHTTPRequestHandler, directory=repo))
        threading.Thread(target=server.serve_forever, daemon=True).start()
        try:
            spec = {"repositories": [f"http://127.0.0.1:{server.server_port}"], "environments": {
                "amd64": {"resolutionContext": {"systemProperties": {"os.arch": "amd64"}}},
                "aarch64": {"resolutionContext": {"systemProperties": {"os.arch": "aarch64"}}},
            }, "classpaths": {"main": {"dependencies": ["t:a:1"]}}}
            (project / "src").mkdir()
            (project / "src/Main.java").write_text("public class Main {}\n")
            (project / "spec.json").write_text(json.dumps(spec))
            (project / "maven.nix").write_text("builtins.fromJSON (builtins.readFile ./spec.json)")
            flake = '''{
              inputs.jix.url = "path:SOURCE";
              inputs.nixpkgs.follows = "jix/nixpkgs";
              outputs = { jix, nixpkgs, ... }: let
                system = "SYSTEM";
                pkgs = import nixpkgs { inherit system; };
                java = jix.lib { inherit pkgs; };
                deps = jix.dependencies { inherit pkgs; spec = import ./maven.nix; lockFile = ./jix.lock; };
                project = jix.project { inherit pkgs deps; forEnvironment = {name, deps}: {
                  targets.jar = java.library { name = "fixture"; sources = ./src; jdk = pkgs.jdk21; release = 21; dependencies = [ deps.classpaths.main ]; };
                  targets.label = pkgs.writeText "environment-${name}" name;
                  apps.label = { type = "app"; program = "${pkgs.writeShellScript "label-${name}" "echo ${name}"}"; };
                  defaultApp = "label";
                }; };
              in {
                jix.${system}.dependencies = project.descriptor;
                packages.${system} = project.packages;
                apps.${system} = project.apps;
              };
            }'''
            system = run("nix", "eval", "--impure", "--raw", "--expr", "builtins.currentSystem").stdout.strip()
            (project / "flake.nix").write_text(flake.replace("SOURCE", str(source)).replace("SYSTEM", system))
            run("nix", "flake", "lock", f"path:{project}")
            run(binary, "lock", "--project", project)
            locked = json.loads((project / "jix.lock").read_text())
            assert locked["version"] == 5
            assert sum(path.endswith(".jar") for path in locked["provenance"]) == 3
            normalized = json.loads(run("nix", "eval", "--json", f"path:{project}#jix.{system}.dependencies.spec").stdout)
            assert normalized == locked["request"]
            nullable = json.loads(json.dumps(spec))
            nullable["environments"]["amd64"]["classpaths"] = None
            (project / "spec.json").write_text(json.dumps(nullable))
            assert json.loads(run("nix", "eval", "--json", f"path:{project}#jix.{system}.dependencies.spec").stdout) == normalized
            (project / "spec.json").write_text(json.dumps(spec))
            original_flake = (project / "flake.nix").read_text()
            # The complete matrix spec must not force the Java callback.
            (project / "flake.nix").write_text(original_flake.replace("forEnvironment = {name, deps}: {", 'forEnvironment = {name, deps}: builtins.seq (throw "build recipes must not be evaluated while locking") {'))
            run(binary, "lock", "--project", project)
            (project / "flake.nix").write_text(original_flake)

            mtime = (project / "jix.lock").stat().st_mtime_ns
            run(binary, "lock", "--project", project)
            assert (project / "jix.lock").stat().st_mtime_ns == mtime
            assert "--environment" in run(binary, "build", "--project", project, success=False).stderr
            run(binary, "build", "--project", project, "--environment", "missing", success=False)
            run("nix", "build", f"path:{project}#default", "--no-link", success=False)
            views = {}
            for name, selected, absent in [("amd64", "x", "y"), ("aarch64", "y", "x")]:
                views[name] = pathlib.Path(run(binary, "build", "dependencies", "--project", project, "--environment", name).stdout.strip())
                direct = run("nix", "build", f"path:{project}#{name}--dependencies", "--no-link", "--print-out-paths").stdout.strip()
                assert str(views[name]) == direct
                manifest = json.loads((views[name] / "artifacts.json").read_text())
                assert manifest["selection"]["environment"] == name
                assert f"t/{selected}/1/{selected}-1.jar" in manifest["artifacts"]
                assert f"t/{absent}/1/{absent}-1.jar" not in manifest["artifacts"]
                cp = run(binary, "classpath", "main", "--project", project, "--environment", name).stdout
                assert f"{selected}-1.jar" in cp
                assert (project / f".jix/roots/{name}--all").resolve() == views[name]
                assert run(binary, "run", "--project", project, "--environment", name).stdout == name + "\n"
                assert run("nix", "run", f"path:{project}#{name}--label").stdout == name + "\n"
                built = run(binary, "build", "jar", "--project", project, "--environment", name).stdout.strip()
                assert built == run("nix", "build", f"path:{project}#{name}--jar", "--no-link", "--print-out-paths").stdout.strip()
                run(binary, "idea", "--project", project, "--environment", name)
                idea = "\n".join(p.read_text() for p in (project / ".idea").rglob("*") if p.suffix in [".xml", ".iml"])
                assert f"{selected}-1.jar" in idea and f"{absent}-1.jar" not in idea
            assert "--environment" in run(binary, "idea", "--project", project, success=False).stderr
            # Same POM/JAR paths have exactly the same Nix fetch derivations across variants.
            maps = [json.loads((view / "artifacts.json").read_text())["artifacts"] for view in views.values()]
            assert maps[0]["t/a/1/a-1.jar"] == maps[1]["t/a/1/a-1.jar"]
            run(binary, "export", views["amd64"], "--output", base / "from-manifest")
            run(binary, "export", views["amd64"], "--environment", "aarch64", "--output", base / "bad", success=False)
            exported = base / "export"
            run(binary, "export", "--project", project, "--environment", "amd64", "--output", exported)
            run(binary, "verify", exported / "jix.lock", "--artifacts", exported / "artifacts.json", "--environment", "amd64")
            run(binary, "verify", exported / "jix.lock", "--artifacts", exported / "artifacts.json", "--environment", "aarch64", success=False)
            run(binary, "verify", "--project", project)
            # Defaults select frozen graphs without changing lock inputs.
            assert run(binary, "build", "--project", project, env=dict(os.environ, JIX_ENVIRONMENT="aarch64")).stdout.strip() == str(views["aarch64"])
            before = (project / "jix.lock").read_bytes()
            default_flake = original_flake.replace(
                "inherit pkgs deps; forEnvironment",
                'inherit pkgs deps; defaultEnvironment = "amd64"; forEnvironment')
            (project / "flake.nix").write_text(default_flake)
            arm_env = dict(os.environ, JIX_ENVIRONMENT="aarch64")
            bad_env = dict(os.environ, JIX_ENVIRONMENT="missing")
            assert run(binary, "build", "--project", project).stdout.strip() == str(views["amd64"])
            assert run(binary, "build", "--project", project, env=arm_env).stdout.strip() == str(views["aarch64"])
            assert run(binary, "build", "--project", project, "--environment", "amd64", env=arm_env).stdout.strip() == str(views["amd64"])
            assert (project / ".jix/roots/amd64--all").resolve() == views["amd64"]
            assert (project / ".jix/roots/aarch64--all").resolve() == views["aarch64"]
            assert run(binary, "run", "--project", project).stdout == "amd64\n"
            assert run(binary, "run", "--project", project, env=arm_env).stdout == "aarch64\n"
            assert "y-1.jar" in run(binary, "classpath", "main", "--project", project, env=arm_env).stdout
            run(binary, "idea", "--project", project, env=arm_env)
            idea = "\n".join(p.read_text() for p in (project / ".idea").rglob("*") if p.suffix in [".xml", ".iml"])
            assert "y-1.jar" in idea and "x-1.jar" not in idea
            run(binary, "export", "--project", project, "--output", base / "implicit", env=arm_env)
            assert json.loads((base / "implicit/artifacts.json").read_text())["selection"]["environment"] == "aarch64"
            run(binary, "export", views["amd64"], "--output", base / "wrong-env", env=arm_env, success=False)
            assert run("nix", "build", f"path:{project}#default", "--no-link", "--print-out-paths", env=arm_env).stdout.strip() == str(views["amd64"])
            assert run("nix", "run", f"path:{project}", env=arm_env).stdout == "amd64\n"
            for value in ["", "missing"]:
                error = run(binary, "build", "--project", project, env=dict(os.environ, JIX_ENVIRONMENT=value), success=False).stderr
                assert "amd64" in error and "aarch64" in error
            for command in ["lock", "info", "verify"]:
                run(binary, command, "--project", project, env=bad_env)
            assert (project / "jix.lock").read_bytes() == before
            # Defaults must stay lazy for locking and explicit selection.
            (project / "flake.nix").write_text(default_flake.replace('defaultEnvironment = "amd64"', 'defaultEnvironment = throw "do not evaluate default"'))
            run(binary, "lock", "--project", project, env=bad_env)
            run(binary, "build", "--project", project, "--environment", "amd64", env=bad_env)
            assert (project / "jix.lock").read_bytes() == before
            (project / "flake.nix").write_text(default_flake.replace('defaultEnvironment = "amd64"', 'defaultEnvironment = "missing"'))
            run(binary, "build", "--project", project, success=False)
            run("nix", "build", f"path:{project}#default", "--no-link", success=False)
            (project / "flake.nix").write_text(default_flake.replace('defaultApp = "label";', ""))
            apps = json.loads(run("nix", "eval", "--json", f"path:{project}#apps.{system}", "--apply", "builtins.attrNames").stdout)
            assert "default" not in apps and "amd64--label" in apps
            (project / "flake.nix").write_text(default_flake.replace('defaultEnvironment = "amd64"', 'defaultEnvironment = null'))
            run(binary, "build", "--project", project, success=False)
            (project / "flake.nix").write_text(original_flake)
            # Workspace normalization is independent of lock existence and Java recipes.
            prefix = f'''let f = builtins.getFlake "path:{source}"; pkgs = import f.inputs.nixpkgs {{ system = "{system}"; }};
              spec = builtins.fromJSON (builtins.readFile {project / 'spec.json'});
              in (f.workspaceDependencies {{ inherit pkgs; specs = SPECS; lockFile = {project / 'missing.lock'}; }}).descriptor.spec'''
            def workspace(specs, success=True):
                return run("nix", "eval", "--impure", "--json", "--expr", prefix.replace("SPECS", specs), success=success)
            merged = json.loads(workspace("{ app = spec; vendor = spec; }").stdout)
            assert list(merged["environments"]["amd64"]["classpaths"]) == ["app.main", "vendor.main"]
            workspace('{ app = spec; vendor = spec // { environments = spec.environments // { amd64 = { resolutionContext = {}; }; }; }; }', success=False)
            workspace('{ app = spec; vendor = { inherit (spec) repositories classpaths; }; }', success=False)
            expanded = json.loads(json.dumps(spec))
            expanded["environments"]["amd64"]["classpaths"] = {"test": {"scope": "test", "dependencies": ["t:x:1"]}}
            (project / "spec.json").write_text(json.dumps(expanded))
            run(binary, "lock", "--project", project)
            changed = json.loads((project / "jix.lock").read_text())
            assert list(changed["request"]["environments"]["amd64"]["classpaths"]) == ["test"]
            assert changed["classpathInputs"]["aarch64"] == locked["classpathInputs"]["aarch64"]
            (project / "spec.json").write_text(json.dumps(spec))
            (project / "jix.lock").write_text(json.dumps(locked))
            # Tampered context ownership must fail in Nix before any realization.
            broken = json.loads(json.dumps(locked))
            broken["classpathInputs"]["amd64"]["main"]["contextHash"] = "0" * 64
            (project / "jix.lock").write_text(json.dumps(broken))
            run(binary, "build", "--project", project, "--environment", "amd64", success=False)
        finally:
            server.shutdown()
    print("Matrix integration passed: default/explicit selection, normalization, scoped realization/export, flake parity, roots, workspace contexts and tampering")


if __name__ == "__main__":
    main()
