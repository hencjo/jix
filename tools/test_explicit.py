#!/usr/bin/env python3
"""Constructor validation and pure normalization/input semantics; no Maven access."""
import pathlib
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[1]


def evaluate(expression, success=True):
    p = subprocess.run(
        ["nix", "eval", "--impure", "--json", "--option", "allow-import-from-derivation", "false", "--expr", expression],
        capture_output=True, text=True, timeout=300,
    )
    assert (p.returncode == 0) == success, p.stdout + p.stderr
    return p


def main():
    prefix = f"let maven = import {ROOT}/nix/maven.nix; in "
    for arguments in [
        '{ dependencies = []; }',
        '{ dependencies = []; scope = "unknown"; }',
        '{ dependencies = []; scope = "compile"; unexpected = true; }',
        '{ dependencies = "not-a-list"; scope = "compile"; }',
    ]:
        evaluate(prefix + "maven.resolve " + arguments, success=False)
    for scope in ["compile", "runtime", "test"]:
        evaluate(prefix + f'maven.resolve {{ dependencies = []; scope = "{scope}"; }}')
    evaluate(f'''let f = builtins.getFlake "path:{ROOT}";
      in f.checks.${{builtins.currentSystem}}.explicit-inputs.drvPath''')
    print("Explicit resolution/input checks passed")


if __name__ == "__main__":
    main()
