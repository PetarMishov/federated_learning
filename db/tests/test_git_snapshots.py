"""Exercise the same Git repository layout and refs as the Rust API."""
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "scripts" / "lib"))
from git_snapshots import capture_commit, git, prepare_versions, publish_commit


class GitSnapshotTests(unittest.TestCase):
    def test_snapshots_retain_exact_file_sets_after_source_removal(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with tempfile.TemporaryDirectory() as sources:
                versions = prepare_versions(Path(sources))
                for snapshot, version in [(7, "v1"), (8, "v2")]:
                    source, sha = versions[version]
                    publish_commit(root, 42, snapshot, source, sha)
            repository = root / "projects/42.git"
            for snapshot, rounds in [(7, 3), (8, 5)]:
                ref = f"refs/snapshots/{snapshot}"
                self.assertEqual(set(git("--git-dir", repository, "ls-tree", "-r", "--name-only", ref).splitlines()),
                    {"README.md", "config.json", "train.py"})
                content = git("--git-dir", repository, "show", f"{ref}:config.json")
                self.assertEqual(json.loads(content)["rounds"], rounds)

    def test_commits_are_reproducible_and_refs_cannot_be_replaced(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            first = prepare_versions(root / "first")
            second = prepare_versions(root / "second")
            self.assertEqual(first["v1"][1], second["v1"][1])
            self.assertNotEqual(first["v1"][1], first["v2"][1])
            source, sha = first["v1"]
            publish_commit(root / "git", 42, 7, source, sha)
            for directory in (root / "git", root / "git/projects", root / "git/projects/42.git"):
                self.assertEqual(directory.stat().st_mode & 0o777, 0o700)
            publish_commit(root / "git", 42, 7, source, sha)
            with self.assertRaises(ValueError):
                publish_commit(root / "git", 42, 7, *first["v2"])
            self.assertEqual(git("--git-dir", root / "git/projects/42.git", "rev-parse", "refs/snapshots/7"), sha)

    def test_storage_and_source_symlinks_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            versions = prepare_versions(root / "sources")
            outside = root / "outside"
            outside.mkdir()
            storage = root / "git"
            storage.mkdir()
            (storage / "projects").symlink_to(outside, target_is_directory=True)
            with self.assertRaises(ValueError):
                publish_commit(storage, 42, 7, *versions["v1"])
            self.assertEqual(list(outside.iterdir()), [])
            source = root / "bad-source"
            source.mkdir()
            (source / "secret").symlink_to(root / "outside")
            with self.assertRaises(ValueError):
                capture_commit(source, root / "worktree", "Bad snapshot")


if __name__ == "__main__":
    unittest.main()
