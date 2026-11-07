#!/usr/bin/env python3
"""Release tests never contact GitHub or publish this repository."""
import contextlib
import importlib.machinery
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
loader = importlib.machinery.SourceFileLoader("release", str(ROOT / "scripts/release"))
spec = importlib.util.spec_from_loader(loader.name, loader)
r = importlib.util.module_from_spec(spec)
loader.exec_module(r)


class Changelog(unittest.TestCase):
    def plan(self, category, old="0.1.0", **kwargs):
        return r.release_plan(f"# Changelog\n\n## [Unreleased]\n\n### {category}\n- An entry\n", old, **kwargs)

    def test_bumps(self):
        for cat in ("Added", "Changed", "Breaking changes"):
            self.assertEqual(self.plan(cat)[0], "0.2.0")
        for cat in ("Fixed", "Security", "Documentation"):
            self.assertEqual(self.plan(cat)[0], "0.1.1")
        self.assertEqual(self.plan("Breaking changes", "1.2.3")[0], "2.0.0")

    def test_versions(self):
        for v in ("01.2.3", "1.2", "v1.2.3", "1.2.3-rc.1", "1.2.3+foo"):
            with self.assertRaises(ValueError):
                r.version(v)
        with self.assertRaises(ValueError):
            self.plan("Added", explicit="0.1.9")
        self.assertEqual(self.plan("Added", explicit="1.0.0")[0], "1.0.0")

    def test_explicit_initial_release(self):
        target, notes, updated = self.plan("Added", explicit="0.1.0", initial=True)
        self.assertEqual(target, "0.1.0")
        self.assertEqual(r.notes_for(updated, target), notes)
        self.assertEqual(self.plan("Fixed", initial=True)[0], "0.1.0")
        with self.assertRaises(ValueError):
            self.plan("Added", explicit="0.2.0", initial=True)
        with self.assertRaises(ValueError):
            r.release_plan(updated, "0.1.0", initial=True)

    def test_empty_and_malformed(self):
        for body in ("", "### Added\n", "### Upgrade notes\n- Only notes\n",
                     "### Surprise\n- x", "### Fixed\n- x\n### Fixed\n- y",
                     "### Added\n<!-- placeholder -->\n", "### Added\n<!--\n",
                     "### Added\n```\n", "### Added\n-->\n",
                     "### Added\n- x\n## [Unreleased]\n"):
            with self.subTest(body=body), self.assertRaises(ValueError):
                r.release_plan("# Changelog\n\n## [Unreleased]\n\n" + body, "0.1.0")

    def test_history_and_placeholders(self):
        history = "## [0.1.0] - 2026-01-01\n\nHistorical {{version}} stays.\n"
        text = "# Changelog\n\n## [Unreleased]\n\n### Added\n- Version {{version}} on {{date}}.\n\n" + history
        v, notes, updated = r.release_plan(text, "0.1.0")
        self.assertTrue(updated.endswith(history))
        self.assertIn("Version 0.2.0", notes)
        self.assertEqual(r.notes_for(updated, v), notes)
        self.assertEqual(updated.count("## [Unreleased]"), 1)

    def test_fenced_headers_and_highest_category(self):
        text = "## [Unreleased]\n\n### Fixed\n- Fix\n```md\n## [fake]\n### Unknown\n```\n### Added\n- Feature\n"
        v, notes, updated = r.release_plan(text, "0.1.0")
        self.assertEqual(v, "0.2.0")
        self.assertEqual(r.notes_for(updated, v), notes)


class Repository(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.oldcwd = Path.cwd()
        self.addCleanup(os.chdir, self.oldcwd)
        self.base = Path(self.temp.name)
        self.repo = self.base / "repo"
        self.repo.mkdir()
        os.chdir(self.repo)
        r.git("init", "-q", "-b", "trunk")
        r.git("config", "user.email", "test@example.invalid")
        r.git("config", "user.name", "Release test")
        r.git("config", "commit.gpgSign", "false")
        r.git("config", "tag.gpgSign", "false")
        r.git("config", "core.hooksPath", "/dev/null")
        Path("Cargo.toml").write_text('[workspace]\nmembers = ["app", "library"]\nresolver = "2"\n[workspace.package]\nversion = "0.1.0"\n')
        Path("app/src").mkdir(parents=True)
        Path("app/Cargo.toml").write_text('[package]\nname = "fixture"\nversion.workspace = true\nedition = "2021"\n')
        Path("app/src/main.rs").write_text('fn main() {}\n')
        Path("library/src").mkdir(parents=True)
        Path("library/Cargo.toml").write_text('[package]\nname = "fixture-library"\nversion.workspace = true\nedition = "2021"\n')
        Path("library/src/lib.rs").write_text("pub fn fixture() {}\n")
        Path("CHANGELOG.md").write_text("# Changelog\n\n## [Unreleased]\n\n### Added\n- A feature\n")
        Path(".gitignore").write_text("/dist/\n/target/\n")
        shutil.copy(ROOT / "release.toml", "release.toml")
        r.git("add", ".")
        r.git("commit", "-qm", "feat: fixture")
        r.git("init", "-q", "--bare", "--initial-branch=trunk", self.base / "origin")
        r.git("remote", "add", "origin", self.base / "origin")
        r.git("push", "-qu", "origin", "trunk")

    def test_preview_read_only(self):
        before = {str(p): p.read_bytes() for p in self.repo.rglob("*") if p.is_file() and ".git" not in p.parts}
        with patch.object(r, "ROOT", self.repo), contextlib.redirect_stdout(io.StringIO()), patch.object(r, "preflight") as pre:
            r.main([])
            pre.assert_not_called()
        after = {str(p): p.read_bytes() for p in self.repo.rglob("*") if p.is_file() and ".git" not in p.parts}
        self.assertEqual(before, after)
        self.assertEqual(r.git("status", "--porcelain"), "")

    def test_preflight_clean_branch_remote_tag_guards(self):
        original = r.run
        def local(*args, **kwargs):
            if args[0] == "gh":
                return ""
            return original(*args, **kwargs)
        with patch.object(r, "run", side_effect=local):
            self.assertEqual(r.preflight("0.2.0"), "trunk")
            self.assertEqual(r.preflight("0.1.0", initial=True), "trunk")
            r.git("tag", "jix-v0.0.1")
            with self.assertRaises(ValueError): r.preflight("0.1.0", initial=True)
            r.git("tag", "-d", "jix-v0.0.1")
            Path("untracked").touch()
            with self.assertRaises(ValueError): r.preflight("0.2.0")
            Path("untracked").unlink()
            r.git("tag", "jix-v0.2.0")
            with self.assertRaises(ValueError): r.preflight("0.2.0")
            r.git("tag", "-d", "jix-v0.2.0")
            r.git("commit", "--allow-empty", "-qm", "ahead")
            with self.assertRaises(ValueError): r.preflight("0.2.0")
            r.git("checkout", "-qb", "topic")
            with self.assertRaises(ValueError): r.preflight("0.2.0")

    def release_tag(self):
        _, _, updated = r.release_plan(Path("CHANGELOG.md").read_text(), "0.1.0")
        Path("CHANGELOG.md").write_text(updated)
        Path("Cargo.toml").write_text(Path("Cargo.toml").read_text().replace('"0.1.0"', '"0.2.0"'))
        r.git("add", ".")
        r.git("commit", "-qm", "chore(release): fixture")
        r.git("tag", "-a", "jix-v0.2.0", "-m", "release")
        r.git("push", "-q", "--atomic", "origin", "trunk", "jix-v0.2.0")

    def archive(self):
        dest = Path("dist")
        dest.mkdir()
        meta = dest / "release.json"
        source = dest / "jix-0.2.0-source.tar.gz"
        source.write_bytes(b"fixture source")
        r.write_checksum(source)
        meta.write_text(json.dumps({"tag": "jix-v0.2.0", "commit": r.git("rev-parse", "HEAD"),
                                   "sourceArchive":source.name,"sourceSha256":r.file_hash(source)}))
        archive = dest / "fixture.tar.gz"
        with tarfile.open(archive, "w:gz") as f:
            f.add(meta, arcname="fixture/release.json")
        archive.with_name(archive.name + ".sha256").write_text(f"{r.file_hash(archive)}  {archive.name}\n")
        return archive

    def test_upload_draft_order_retry_and_failures(self):
        self.release_tag()
        archive = self.archive()
        original = r.run
        calls = []
        existing = []
        failure = None
        def fake(*args, **kwargs):
            if args[0] != "gh": return original(*args, **kwargs)
            calls.append(args)
            if args[1] == "repo": return '{"nameWithOwner":"owner/repo"}'
            if args[1] == "api": return json.dumps([existing])
            if args[2] == failure: raise subprocess.CalledProcessError(1, args)
            return ""
        with patch.object(r, "run", side_effect=fake):
            r.upload("jix-v0.2.0", archive)
            self.assertEqual([c[2] for c in calls if c[1] == "release"], ["create", "upload", "edit"])
            assets = next(c for c in calls if c[1:3] == ("release", "upload"))
            self.assertIn(archive.parent / "jix-0.2.0-source.tar.gz", assets)
            self.assertIn(archive.parent / "jix-0.2.0-source.tar.gz.sha256", assets)
            existing.append({"tag_name": "jix-v0.2.0", "draft": True})
            calls.clear()
            r.upload("jix-v0.2.0", archive)
            self.assertEqual([c[2] for c in calls if c[1] == "release"], ["upload", "edit"])
            calls.clear()
            failure = "upload"
            with self.assertRaises(subprocess.CalledProcessError): r.upload("jix-v0.2.0", archive)
            self.assertNotIn("edit", [c[2] for c in calls if c[1] == "release"])
            failure = None
            existing[0]["draft"] = False
            with self.assertRaises(ValueError): r.upload("jix-v0.2.0", archive)
            archive.write_bytes(b"corrupt")
            with self.assertRaises(ValueError): r.upload("jix-v0.2.0", archive)

    def test_upload_rejects_wrong_source_before_github(self):
        self.release_tag()
        archive = self.archive()
        source = archive.parent / "jix-0.2.0-source.tar.gz"
        source.write_bytes(b"different source")
        r.write_checksum(source)
        original = r.run
        def local(*args, **kwargs):
            self.assertNotEqual(args[0], "gh", "Invalid assets must not contact GitHub")
            return original(*args, **kwargs)
        with patch.object(r, "run", side_effect=local), self.assertRaisesRegex(ValueError, "Source archive"):
            r.upload("jix-v0.2.0", archive)

    def test_build_package_and_checksum(self):
        for name in ("README.md", "LICENSE", "COMPATIBILITY.md"):
            Path(name).write_text("fixture\n")
        r.git("add", ".")
        r.git("commit", "-qm", "docs: packaging fixture")
        self.release_tag()
        original_run, original_subprocess = r.run, subprocess.run
        calls = []
        def fake_run(*args, **kwargs):
            if args[0] == "bash":
                calls.append("checks")
                return ""
            if args[:2] == ("nix", "eval"): return "x86_64-linux"
            if args[:2] == ("nix", "build"):
                calls.append("build")
                return "/nix/store/fixture-jix"
            if str(args[0]).endswith("/bin/jix"): return "jix 0.2.0"
            if args[0] == "nix-store": return "/nix/store/fixture-jix"
            return original_run(*args, **kwargs)
        def fake_process(args, **kwargs):
            if args[0] == "nix-store":
                kwargs["stdout"].write(b"fixture closure")
                return subprocess.CompletedProcess(args, 0)
            return original_subprocess(args, **kwargs)
        with patch.object(r, "run", side_effect=fake_run), patch.object(subprocess, "run", side_effect=fake_process):
            archive = r.build("jix-v0.2.0")
        self.assertEqual(calls, ["checks", "build"])
        with tarfile.open(archive) as f:
            names = f.getnames()
            self.assertTrue(any(n.endswith("/closure.nar") for n in names))
            self.assertTrue(any(n.endswith("/source.tar.gz") for n in names))
            meta = next(n for n in names if n.endswith("/release.json"))
            metadata = json.load(f.extractfile(meta))
            self.assertEqual(metadata["commit"], r.git("rev-parse", "HEAD"))
            source = archive.parent / metadata["sourceArchive"]
            self.assertEqual(metadata["sourceSha256"], r.file_hash(source))
            bundled_source = next(n for n in names if n.endswith("/source.tar.gz"))
            self.assertEqual(f.extractfile(bundled_source).read(), source.read_bytes())
        with tarfile.open(source) as f:
            self.assertIn("jix-0.2.0/Cargo.toml", f.getnames())
            self.assertIn("jix-0.2.0/CHANGELOG.md", f.getnames())
        self.assertEqual(source.with_name(source.name + ".sha256").read_text(),
                         f"{r.file_hash(source)}  {source.name}\n")
        self.assertEqual(archive.with_name(archive.name + ".sha256").read_text(),
                         f"{r.file_hash(archive)}  {archive.name}\n")

    def test_execute_stops_at_failed_steps(self):
        for failed in ("version", "hook", "commit", "tag", "build", "push"):
            calls = []
            original = Path("CHANGELOG.md").read_text()
            def step(name, *args):
                calls.append(name)
                if name == failed: raise ValueError("injected " + name)
            def run(*args, **kwargs):
                if args[:2] == ("git", "push"): step("push")
            with patch.object(r, "ROOT", self.repo), patch.object(r, "preflight", return_value="trunk"), \
                 patch.object(r, "cargo_release", side_effect=step), patch.object(r, "build", side_effect=lambda _: step("build")), \
                 patch.object(r, "run", side_effect=run), patch.object(r, "upload") as upload, contextlib.redirect_stdout(io.StringIO()):
                with self.assertRaises(ValueError): r.main(["--execute"])
                self.assertEqual(calls[-1], failed)
                upload.assert_not_called()
            Path("CHANGELOG.md").write_text(original)

    @unittest.skipUnless(shutil.which("cargo-release") and shutil.which("cargo"), "requires pinned cargo-release")
    def test_real_cargo_release_without_registry_publication(self):
        r.run("cargo", "generate-lockfile", "--offline")
        r.git("add", "Cargo.lock")
        r.git("commit", "-qm", "chore: lock")
        r.cargo_release("version", "trunk", "0.2.0", "--workspace")
        _, _, updated = r.release_plan(Path("CHANGELOG.md").read_text(), "0.1.0")
        Path("CHANGELOG.md").write_text(updated)
        r.cargo_release("hook", "trunk", "--workspace")
        r.cargo_release("commit", "trunk")
        r.cargo_release("tag", "trunk", "--workspace")
        self.assertEqual(r.verify_tag("jix-v0.2.0"), "0.2.0")

    @unittest.skipUnless(shutil.which("cargo-release") and shutil.which("cargo"), "requires pinned cargo-release")
    def test_real_initial_release_keeps_cargo_version(self):
        r.run("cargo", "generate-lockfile", "--offline")
        r.git("add", "Cargo.lock")
        r.git("commit", "-qm", "chore: lock")
        manifests = {p: p.read_bytes() for p in (Path("Cargo.toml"), Path("Cargo.lock"))}
        _, _, updated = r.release_plan(Path("CHANGELOG.md").read_text(), "0.1.0", initial=True)
        Path("CHANGELOG.md").write_text(updated)
        r.cargo_release("hook", "trunk", "--workspace")
        r.cargo_release("commit", "trunk")
        r.cargo_release("tag", "trunk", "--workspace")
        self.assertEqual(r.verify_tag("jix-v0.1.0"), "0.1.0")
        for path, original in manifests.items():
            self.assertEqual(path.read_bytes(), original)


if __name__ == "__main__":
    unittest.main()
