#!/usr/bin/env python3
"""Downstream callPackage, profile installation and bundled-source acceptance."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def run(*args, success=True, **kwargs):
    result = subprocess.run([str(a) for a in args], text=True, capture_output=True,
                            timeout=900, **kwargs)
    if (result.returncode == 0) != success:
        raise AssertionError(f"{args}\n{result.stdout}\n{result.stderr}")
    return result


def main():
    # Resolve the Git-filtered flake source, not an unfiltered checkout snapshot.
    reference = f"git+file://{ROOT}" if (ROOT / ".git").exists() else f"path:{ROOT}"
    metadata = json.loads(run("nix", "flake", "metadata", "--json",
                              "--no-update-lock-file", reference).stdout)
    source = metadata["path"]
    nixpkgs = run("nix", "eval", "--impure", "--raw", "--expr",
                  f'(builtins.getFlake "path:{source}").inputs.nixpkgs.outPath').stdout.strip()
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    with tempfile.TemporaryDirectory(prefix="jix-distribution-") as tmp:
        base = Path(tmp)
        recipe = base / "package.nix"
        # A derivation source models fetchFromGitHub without fetching an unreleased
        # tag. Evaluation must not inspect this source (IFD is disabled below).
        recipe.write_text(f'''{{ wrongNix ? false }}:
let
  pkgs = import {nixpkgs} {{ }};
  src = pkgs.runCommand "jix-downstream-source" {{ upstream = {source}; }} "cp -r $upstream $out";
in pkgs.callPackage {source}/nix/package.nix {{
  inherit src;
  version = "{version}";
  nix = if wrongNix then pkgs.nix // {{ version = "0.0.0"; }} else pkgs.nix;
  oracle = import {source}/nix/oracle.nix {{ inherit pkgs; }};
}}
''')
        failed = run("nix", "eval", "--raw", "--file", recipe, "drvPath",
                     "--arg", "wrongNix", "true", success=False)
        assert "update and test" in failed.stderr, failed.stderr
        package = run("nix", "build", "--file", recipe, "--no-link", "--print-out-paths",
                      "--option", "allow-import-from-derivation", "false").stdout.strip()
        bundled_source = run("nix", "eval", "--raw", "--file", recipe,
                             "JIX_FLAKE_SOURCE").stdout.strip().removeprefix("path:")
        closure = run("nix-store", "--query", "--requisites", package).stdout.splitlines()
        assert bundled_source in closure, "init's bundled source is not retained in the runtime closure"
        profile = base / "profile"
        run("nix", "profile", "install", "--profile", profile, package)
        binary = profile / "bin/jix"
        assert run(binary, "--version").stdout.strip() == f"jix {version}"
        env = dict(os.environ, RUST_MIN_STACK="2097152")
        project = base / "app"
        # No --jix-source: installed binaries must bootstrap without their checkout.
        run(binary, "init", "--project", project, "--template", "thin", env=env, cwd=base)
        assert f"path:{bundled_source}" in (project / "flake.nix").read_text()
        run(binary, "lock", "--project", project, env=env)
        run(binary, "verify", "--project", project, env=env)
        run(binary, "build", "--project", project, env=env)
        assert run(binary, "run", "--project", project, env=env).stdout == "Hello from Jix!\n"

        # The README's flake development-shell and nativeBuildInputs examples.
        consumer = base / "consumer"
        consumer.mkdir()
        readme = (ROOT / "README.md").read_text().split("## Use Jix from your flake", 1)[1]
        example = readme.split("```nix\n", 1)[1].split("```", 1)[0]
        example = example.replace("github:hencjo/jix", f"path:{source}")
        example = example.replace("github:NixOS/nixpkgs/nixos-unstable", f"path:{nixpkgs}")
        (consumer / "flake.nix").write_text(example)
        guide = run("nix", "build", f"path:{consumer}#migration-guide", "--no-link",
                    "--print-out-paths").stdout.strip()
        assert Path(guide).read_text() == run(binary, "migrate", "--guide").stdout
        assert run("nix", "develop", f"path:{consumer}", "--command", "jix",
                   "--version").stdout.strip() == f"jix {version}"
    print("Distribution passed: callPackage without IFD, native pin, profile, bundled init, flake inputs")


if __name__ == "__main__":
    main()
