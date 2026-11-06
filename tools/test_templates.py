#!/usr/bin/env python3
"""Host gate: initialize, Nix-build/run and safely regenerate native IDEA projects."""
import os
import functools
import http.server
import threading
import zipfile
import pathlib
import shutil
import tempfile
import xml.etree.ElementTree as ET
from test_project import ROOT, run

def main():
    binary = os.environ.get("JIX_TEST_BINARY")
    if not binary:
        package = run("nix", "build", "path:.#jix", "--no-link", "--print-out-paths", cwd=ROOT).stdout.strip()
        binary = str(pathlib.Path(package) / "bin/jix")
    binary = str(pathlib.Path(binary).resolve())
    with tempfile.TemporaryDirectory(prefix="jix-templates-") as tmp:
        root = pathlib.Path(tmp)
        repository = root / "repository"
        artifact = repository / "test/fixture/1/fixture-1"
        artifact.parent.mkdir(parents=True)
        artifact.with_suffix(".pom").write_text("<project><modelVersion>4.0.0</modelVersion><groupId>test</groupId><artifactId>fixture</artifactId><version>1</version></project>")
        with zipfile.ZipFile(artifact.with_suffix(".jar"), "w") as jar:
            jar.writestr("META-INF/MANIFEST.MF", "Manifest-Version: 1.0\r\nAutomatic-Module-Name: fixture.dependency\r\n\r\n")
        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(http.server.SimpleHTTPRequestHandler, directory=repository))
        threading.Thread(target=server.serve_forever, daemon=True).start()
        source = root / "source"
        shutil.copytree(ROOT, source, ignore=shutil.ignore_patterns(".aws", ".codex", ".agents", "dist", ".git", ".jix", ".idea", ".devenv*", ".direnv", "target", "result", "__pycache__"))
        source = run("nix", "store", "add-path", source).stdout.strip()
        for template in ("thin", "fat", "shadow", "multi-module", "modules"):
            project = root / template
            run(binary, "init", "--project", project, "--template", "multi-module" if template == "modules" else template, "--jix-source", f"path:{source}")
            if template == "modules":
                build = project / "build.nix"
                build.write_text(build.read_text().replace("classpath =", "modulepath =").replace("runtime.classpath", "runtime.modulepath")
                    .replace('mainClass = "example.Main";', 'module = "example.app/example.Main";'))
                (project / "core/src/main/java/module-info.java").write_text("module example.core { exports example.core; }")
                (project / "app/src/main/java/module-info.java").write_text("module example.app { requires example.core; }")
            if template == "thin":
                (project / "src/main/java").rename(project / "src/main/java space & #%")
                build = project / "build.nix"
                build.write_text(build.read_text().replace("./src/main/java", '(./src/main + "/java space & #%")'))
                (project / "maven.nix").write_text('{ repositories = [ "http://127.0.0.1:' + str(server.server_port) + '" ]; classpaths.main.dependencies = [ "test:fixture:1" ]; classpaths.runtime.dependencies = []; }')
            run(binary, "lock", "--project", project)
            run(binary, "idea", "--project", project)
            if template == "thin":
                aliases = list((project / ".jix/idea-libs").rglob("fixture-1.jar"))
                assert len(aliases) == 1 and aliases[0].is_symlink() and str(aliases[0].resolve()).startswith("/nix/store/")
            for file in (project / ".idea").rglob("*"):
                if file.is_file():
                    ET.parse(file)
            before = {str(p): p.read_bytes() for p in (project / ".idea").rglob("*") if p.is_file()}
            (project / ".idea/workspace.xml").write_text("<project/>")
            run(binary, "idea", "--project", project)
            assert all(pathlib.Path(p).read_bytes() == content for p, content in before.items())
            assert (project / ".idea/workspace.xml").read_text() == "<project/>"
            if idea := os.environ.get("JIX_IDEA_HOME"):
                from idea_smoke import smoke
                smoke(project, idea)
            result = run(binary, "run", "--project", project)
            assert "Hello" in result.stdout, result.stdout
            jar = run(binary, "build", "jar", "--project", project).stdout.strip()
            assert run("nix", "build", f"path:{project}#jar", "--no-link", "--print-out-paths").stdout.strip() == jar
            if template in ("fat", "shadow"):
                with zipfile.ZipFile(jar) as archive:
                    main = "internal/example/Main.class" if template == "shadow" else "example/Main.class"
                    assert main in archive.namelist()
                    assert "META-INF/jix/origins.json" in archive.namelist()
            owned = project / ".idea/compiler.xml"
            owned.write_text("user edit")
            failed = run(binary, "idea", "--project", project, success=False)
            assert "Modified IntelliJ file" in failed.stderr, failed.stderr
            assert owned.read_text() == "user edit"
            print(f"{template}: init, idea, regeneration, build, run, flake parity, ownership passed", flush=True)
        server.shutdown()
    print("Template/IntelliJ integration passed")

if __name__ == "__main__":
    main()
