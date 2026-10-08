import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True
SCRIPTS = Path(__file__).resolve().parents[1] / "scripts"


def load(name):
    spec = importlib.util.spec_from_file_location(name, SCRIPTS / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


commands = load("git_command")
setup = load("setup_server")


class GitServerTests(unittest.TestCase):
    def test_forced_command_rejects_shells_options_and_paths_outside_projects(self):
        for command in [
            "", "id", "sh -c 'id'", "git-upload-pack '/etc/passwd'",
            "git-upload-pack '/repositories/../other.git'",
            "git-upload-pack '/repositories/0.git'",
            "git-upload-pack '/repositories/2147483648.git'",
            "git-upload-pack '/repositories/1.git' --help",
            "git-upload-pack '/repositories/1.git'; id",
            "git-upload-archive '/repositories/1.git'",
            "git-receive-pack 'ssh://other-server/repo.git'",
        ]:
            with self.subTest(command=command), self.assertRaises(ValueError):
                commands.git_arguments(command)

    def test_setup_reuses_keys_and_derives_matching_public_keys(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "id_ed25519"
            first = setup.identity(path, "test")
            private = path.read_bytes()
            self.assertEqual(first, setup.identity(path, "test"))
            self.assertEqual(private, path.read_bytes())
            self.assertEqual(path.with_suffix(".pub").read_text(), first + "\n")
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)

    def test_setup_rejects_symlink_storage_and_key_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            outside = root / "outside"
            outside.mkdir()
            (root / "storage").symlink_to(outside, target_is_directory=True)
            with self.assertRaises(ValueError):
                setup.private_directory(root / "storage" / "ssh")
            self.assertEqual(list(outside.iterdir()), [])
            (root / "key").symlink_to(outside / "key")
            with self.assertRaises(ValueError):
                setup.identity(root / "key", "test")


if __name__ == "__main__":
    unittest.main()
