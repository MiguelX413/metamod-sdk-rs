import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from artifacts import pin_runtime
from staging import stage


@unittest.skipUnless(os.name == "posix", "requires POSIX symlink and descriptor operations")
class StagingTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name).resolve()
        self.source = self.root / "source"
        self.source.mkdir()

    def test_materializes_internal_relative_and_absolute_links(self):
        folder = self.source / "folder"
        folder.mkdir()
        payload = folder / "payload"
        payload.write_bytes(b"fixture")
        (folder / "relative").symlink_to("../folder/payload")
        (self.source / "dir_alias").symlink_to(folder, target_is_directory=True)
        (self.source / "file_alias").symlink_to(payload)
        destination = self.root / "stage"
        stage(self.source, destination, self.root / "unused")
        self.assertEqual((destination / "dir_alias/relative").read_bytes(), b"fixture")
        self.assertEqual((destination / "file_alias").read_bytes(), b"fixture")
        self.assertFalse(any(path.is_symlink() for path in destination.rglob("*")))
        self.assertTrue((self.source / "file_alias").is_symlink())

    def test_rejects_external_broken_cyclic_and_special_inputs(self):
        outside = self.root / "secret"
        outside.write_text("private")
        for target, reason in ((outside, "escapes"), ("../secret", "escapes"),
                               ("../source", "escapes"), ("not-present", "Unresolved"),
                               ("link", "cycle"), (".", "directory symlink cycle")):
            with self.subTest(target=target):
                link = self.source / "link"
                link.symlink_to(target)
                output = self.root / "stage"
                with self.assertRaisesRegex(ValueError, reason):
                    stage(self.source, output, self.root / "unused")
                self.assertFalse(output.exists())
                link.unlink()
        os.mkfifo(self.source / "pipe")
        with self.assertRaisesRegex(ValueError, "special file"):
            stage(self.source, self.root / "stage", self.root / "unused")


@unittest.skipUnless(os.name == "posix" and shutil.which("git") and shutil.which("bash"),
                     "requires Bash and Git")
class CheckoutTests(unittest.TestCase):
    def test_clean_dirty_mismatched_and_incomplete_gitlinks(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            repo, child = root / "repo", root / "child"

            def git(path, *args):
                return subprocess.check_output(["git", "-c", "commit.gpgsign=false", "-C", str(path), *args],
                                               text=True).strip()

            for path in (repo, child):
                path.mkdir()
                git(path, "init", "-q")
                git(path, "config", "user.email", "test@example.invalid")
                git(path, "config", "user.name", "Test")
                (path / "tracked").write_text("initial")
                git(path, "add", "tracked")
                git(path, "commit", "-qm", "initial")

            helper = Path(__file__).with_name("checkout.sh")

            def check(expected="", allow="0"):
                return subprocess.run(["bash", "-c", 'source "$1"; check_checkout "$2" "$3" "$4"',
                                       "_", str(helper), str(repo), expected, allow],
                                      capture_output=True, text=True)

            self.assertEqual(check(git(repo, "rev-parse", "HEAD")).returncode, 0)
            self.assertIn("mismatch", check("0" * 40).stderr)
            (repo / "tracked").write_text("modified")
            self.assertIn("dirty", check().stderr)
            git(repo, "add", "tracked")
            self.assertIn("dirty", check().stderr)
            git(repo, "restore", "--source=HEAD", "--staged", "--worktree", "tracked")
            (repo / "untracked").touch()
            self.assertIn("dirty", check().stderr)
            (repo / "untracked").unlink()

            git(repo, "-c", "protocol.file.allow=always", "submodule", "add", str(child), "upstream/sdk")
            git(repo, "commit", "-qm", "pin child")
            self.assertEqual(check().returncode, 0)
            git(repo / "upstream/sdk", "-c", "user.email=test@example.invalid", "-c", "user.name=Test",
                "commit", "--allow-empty", "-qm", "moved")
            self.assertNotEqual(check().returncode, 0)
            git(repo, "submodule", "update", "--checkout")
            git(repo, "submodule", "deinit", "-f", "--all")
            self.assertIn("incomplete", check().stderr)
            self.assertEqual(check(allow="1").returncode, 0)


class ArtifactTests(unittest.TestCase):
    def test_saved_artifact_modification_is_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp).resolve()
            names = ("engine", "metamod", "gamedll")
            row = {name: {"binary": name} for name in names}
            row["engine"]["modules"] = ["engine.so"]
            for name in (*names, "engine.so"):
                (root / name).write_text(name)
            pin_runtime(row, root)
            pin_runtime(row, root, {})
            (root / "engine.so").write_text("replacement")
            with self.assertRaisesRegex(ValueError, "engine module changed"):
                pin_runtime(row, root, {})
