"""Exercise staged-file guardrails in isolated Git repositories."""
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

CHECKER = Path(__file__).resolve().parents[3] / "scripts/check-staged.py"


class StagedFileTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name)
        # Hooks may export GIT_INDEX_FILE/GIT_DIR. Never let fixture repositories
        # use the parent checkout's index or Git directory.
        self.environment = {name: value for name, value in os.environ.items() if not name.startswith("GIT_")}
        self.git("init", "-q")

    def tearDown(self):
        self.directory.cleanup()

    def git(self, *args):
        return subprocess.run(["git", *args], cwd=self.root, check=True,
                              capture_output=True, env=self.environment)

    def stage(self, name, content):
        (self.root / name).write_bytes(content)
        self.git("add", "--", name)

    def check(self, *args):
        return subprocess.run(["python3", str(CHECKER), *args], cwd=self.root,
                              capture_output=True, text=True, env=self.environment)

    def test_uses_staged_blob_instead_of_unstaged_fix(self):
        self.stage("code.py", b"broken = True  \n")
        (self.root / "code.py").write_bytes(b"fixed = True\n")
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("trailing whitespace", result.stdout)

    def test_markdown_hard_break_is_allowed_but_extra_spaces_are_not(self):
        self.stage("README.md", b"Paragraph  \nnext line\n")
        self.assertEqual(self.check().returncode, 0)
        self.stage("README.md", b"Paragraph   \n")
        self.assertEqual(self.check().returncode, 1)

    def test_credential_pattern_does_not_echo_value(self):
        credential = b"ghp_" + b"x" * 36
        self.stage("config.txt", credential + b"\n")
        result = self.check("--all")
        self.assertEqual(result.returncode, 1)
        self.assertIn("possible credential", result.stdout)
        self.assertNotIn(credential.decode(), result.stdout + result.stderr)

    def test_environment_files_and_merge_markers_are_rejected(self):
        self.stage(".env", b"placeholder\n")
        self.stage("conflict.txt", b"<<<<<<< local\nours\n=======\ntheirs\n>>>>>>> remote\n")
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("credential file", result.stdout)
        self.assertIn("merge marker", result.stdout)

    def test_deleted_index_paths_are_skipped_and_example_is_allowed(self):
        self.stage(".env.example", b"SMTP_PASSWORD=\n")
        self.stage("removed.txt", b"bad  \n")
        self.git("rm", "--cached", "removed.txt")
        self.assertEqual(self.check().returncode, 0)

    @unittest.skipUnless(os.name == "posix", "symlink check requires POSIX")
    def test_symlinks_are_rejected_before_export_checks(self):
        os.symlink("/outside-repository", self.root / "external")
        self.git("add", "external")
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("regular tracked files", result.stdout)


if __name__ == "__main__":
    unittest.main()
