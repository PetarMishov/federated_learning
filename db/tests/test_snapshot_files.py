"""Storage checks use temporary directories only."""
import hashlib
import io
import json
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "scripts" / "lib"))
from snapshots import archive_bytes, write_once


class SnapshotFilesTests(unittest.TestCase):
    def test_snapshots_are_reproducible_exact_file_sets_with_visible_changes(self):
        first = archive_bytes("v1")
        updated = archive_bytes("v2")
        self.assertEqual(first, archive_bytes("v1"))
        self.assertNotEqual(hashlib.sha256(first).digest(), hashlib.sha256(updated).digest())
        for content, rounds in [(first, 3), (updated, 5)]:
            with tarfile.open(fileobj=io.BytesIO(content)) as archive:
                self.assertEqual(set(archive.getnames()), {"README.md", "config.json", "train.py"})
                self.assertEqual(json.load(archive.extractfile("config.json"))["rounds"], rounds)

    def test_existing_snapshots_are_not_overwritten(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "snapshots"
            key = Path("demo/example/v1.tar")
            content = archive_bytes("v1")
            write_once(root, key, content)
            write_once(root, key, content)
            self.assertEqual((root / key).stat().st_mode & 0o777, 0o400)
            with self.assertRaises(ValueError):
                write_once(root, key, archive_bytes("v2"))
            self.assertEqual((root / key).read_bytes(), content)

    def test_storage_symlinks_are_rejected_before_creating_external_directories(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "snapshots"
            outside = Path(directory) / "outside"
            root.mkdir()
            outside.mkdir()
            (root / "demo").symlink_to(outside, target_is_directory=True)
            with self.assertRaises(ValueError):
                write_once(root, Path("demo/example/v1.tar"), archive_bytes("v1"))
            self.assertEqual(list(outside.iterdir()), [])


if __name__ == "__main__":
    unittest.main()
