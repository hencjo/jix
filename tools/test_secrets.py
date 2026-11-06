"""Exercise secret scanning against disposable repositories and synthetic tokens."""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


@unittest.skipUnless(shutil.which("gitleaks"), "requires devenv Gitleaks")
class SecretScanTests(unittest.TestCase):
    def test_scan_coverage_and_redaction(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "scripts").mkdir()
            shutil.copy(ROOT / "scripts/check-secrets", root / "scripts/check-secrets")
            shutil.copy(ROOT / ".gitleaks.toml", root)
            env = dict(os.environ, GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)

            def git(*args):
                subprocess.run(["git", *args], cwd=root, env=env, check=True,
                               stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)

            def scan(expected):
                result = subprocess.run([sys.executable, "scripts/check-secrets"],
                                        cwd=root, env=env, text=True, capture_output=True)
                self.assertEqual(result.returncode, expected, result.stderr)
                self.assertNotIn(token, result.stdout + result.stderr)

            token = "ghp_" + "Ab9Cd8Ef7Gh6Ij5Kl4Mn3Op2Qr1St0Uv9Wx8"
            git("init", "-q")
            git("config", "user.name", "Test")
            git("config", "user.email", "test@example.org")
            git("add", ".")
            git("commit", "-qm", "clean")
            scan(0)
            secret = root / "credential.txt"
            secret.write_text(token + "\n")
            scan(1)  # Untracked worktree content.
            git("add", "credential.txt")
            secret.write_text("clean\n")
            scan(1)  # Secret only in the index.
            git("commit", "-qm", "synthetic fixture")
            git("add", "credential.txt")
            git("commit", "-qm", "remove fixture")
            scan(1)  # Secret only in history.


if __name__ == "__main__":
    unittest.main()
