#!/usr/bin/env python3
"""Compare both implementations; optionally minimize a successful-resolution mismatch."""
import argparse
import json
import pathlib
import re
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]


def compare(request):
    reports = []
    for command in ([str(ROOT / "target/debug/jix"), "resolve", str(request)],
                    [str(ROOT / "tools/oracle/run.sh"), str(request)]):
        result = subprocess.run(command, capture_output=True, text=True, timeout=30)
        if result.returncode:
            raise RuntimeError(f"{' '.join(command)} failed:\n{result.stdout}\n{result.stderr}")
        reports.append(json.loads(result.stdout))
    return reports[0] == reports[1], reports


def minimize(source, destination):
    """Greedily remove roots and POM dependency entries, preserving a semantic mismatch.

    Never modifies the input repository. Requires both resolvers to keep succeeding;
    turning a mismatch into a missing-POM error is not a valid reduction.
    """
    if destination.exists():
        raise ValueError(f"Refusing to overwrite {destination}")
    request = json.loads(source.read_text())
    for url in request["repositories"]:
        if not url.startswith("file:///"):
            raise ValueError("Minimization requires absolute file:// fixture repositories")
    destination.mkdir(parents=True)
    repositories = []
    for index, url in enumerate(request["repositories"]):
        repo = destination / f"repository-{index}"
        original = pathlib.Path(url.removeprefix("file://")).resolve()
        if destination == original or destination.is_relative_to(original):
            raise ValueError("Destination must be outside input repositories")
        shutil.copytree(original, repo, symlinks=True)
        repositories.append(repo.as_uri())
    request["repositories"] = repositories
    output = destination / "request.json"

    def mismatch():
        output.write_text(json.dumps(request, indent=2) + "\n")
        try:
            equal, _ = compare(output)
            return not equal
        except RuntimeError:
            return False

    if not mismatch():
        raise ValueError("Input is not a successful-resolution mismatch")
    changed = True
    while changed:
        changed = False
        for cp in request["classpaths"].values():
            for key in ("dependencies", "platforms"):
                values = cp.get(key, [])
                index = 0
                while index < len(values):
                    removed = values.pop(index)
                    if mismatch():
                        changed = True
                    else:
                        values.insert(index, removed)
                        index += 1
        for pom in sorted(destination.glob("repository-*/**/*.pom")):
            original = pom.read_text()
            # Preserve upstream bytes outside the removed dependency element.
            for match in reversed(list(re.finditer(r"<dependency>.*?</dependency>", original, re.S))):
                candidate = original[:match.start()] + original[match.end():]
                pom.write_text(candidate)
                if mismatch():
                    original = candidate
                    changed = True
                else:
                    pom.write_text(original)
    mismatch()  # Persist the final request, not the last rejected candidate.
    _, reports = compare(output)
    for name, report in zip(("rust", "java"), reports):
        (destination / f"{name}.json").write_text(json.dumps(report, indent=2) + "\n")
    print(output)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("request", type=pathlib.Path)
    parser.add_argument("--minimize", type=pathlib.Path, metavar="NEW_DIRECTORY")
    args = parser.parse_args()
    if args.minimize:
        minimize(args.request.resolve(), args.minimize.resolve())
        return 0
    equal, reports = compare(args.request.resolve())
    if equal:
        print("Maven/Rust reports match")
        return 0
    print(json.dumps({"rust": reports[0], "java": reports[1]}, indent=2))
    return 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, RuntimeError, subprocess.TimeoutExpired) as error:
        print(f"error: {error}", file=sys.stderr)
        sys.exit(2)
