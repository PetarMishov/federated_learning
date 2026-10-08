#!/usr/bin/env python3
"""Prepare demo Git commits and retain them in the API's project repositories."""
import csv
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


def reject_symlinks(path: Path) -> None:
    for component in (path, *path.parents):
        if component.is_symlink():
            raise ValueError(f"Git storage cannot contain symlinks: {component}")


def git(*args: object, **overrides: str) -> str:
    env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL="/dev/null", GIT_TERMINAL_PROMPT="0")
    env.update(overrides)
    return subprocess.run(
        ["git", "-c", "core.hooksPath=/dev/null", "-c", "protocol.ext.allow=never", *map(str, args)],
        env=env, check=True, capture_output=True, text=True, timeout=30,
    ).stdout.strip()


def capture_commit(source: Path, worktree: Path, message: str) -> str:
    """Capture complete file contents; source needs no existing Git history."""
    reject_symlinks(source)
    for entry in source.rglob("*"):
        if entry.is_symlink() or (not entry.is_file() and not entry.is_dir()):
            raise ValueError(f"Unsupported snapshot entry: {entry}")
        if entry.name == ".git":
            raise ValueError("Snapshot contents must exclude Git metadata")
    shutil.copytree(source, worktree)
    git("init", "--quiet", "--template=", "--object-format=sha1", worktree)
    git("-C", worktree, "add", "--all", "--force")
    tree = git("-C", worktree, "write-tree")
    return git(
        "-C", worktree, "commit-tree", tree, "-m", message,
        GIT_AUTHOR_NAME="Snapshot storage", GIT_AUTHOR_EMAIL="snapshots@localhost",
        GIT_COMMITTER_NAME="Snapshot storage", GIT_COMMITTER_EMAIL="snapshots@localhost",
        GIT_AUTHOR_DATE="2000-01-01T00:00:00Z", GIT_COMMITTER_DATE="2000-01-01T00:00:00Z",
    )


def publish_commit(root: Path, project_id: int, snapshot_id: int, source: Path, sha: str) -> None:
    if not root.is_absolute() or project_id <= 0 or snapshot_id <= 0:
        raise ValueError("Absolute storage root and positive IDs required")
    repository = root / "projects" / f"{project_id}.git"
    reject_symlinks(repository)
    root.mkdir(parents=True, exist_ok=True, mode=0o700)
    repository.parent.mkdir(exist_ok=True, mode=0o700)
    repository.mkdir(exist_ok=True, mode=0o700)
    if not (repository / "HEAD").exists():
        git("init", "--bare", "--quiet", "--template=", "--object-format=sha1", repository)
    if git("--git-dir", repository, "rev-parse", "--is-bare-repository") != "true":
        raise ValueError("Project storage must be a bare Git repository")
    ref = f"refs/snapshots/{snapshot_id}"
    refs = git("--git-dir", repository, "for-each-ref", "--format=%(objectname) %(refname)", ref)
    existing = next((line.split()[0] for line in refs.splitlines() if line.split()[1] == ref), None)
    if existing is not None:
        if existing != sha:
            raise ValueError(f"Refusing to replace snapshot {snapshot_id}")
        if git("--git-dir", repository, "cat-file", "-t", sha) != "commit":
            raise ValueError("Snapshots must retain commits")
        return
    git("--git-dir", repository, "fetch", "--quiet", "--no-tags", "--no-write-fetch-head", source, sha)
    if git("--git-dir", repository, "cat-file", "-t", sha) != "commit":
        raise ValueError("Snapshots must reference commits")
    git("--git-dir", repository, "update-ref", ref, sha, "0" * 40)


def prepare_versions(directory: Path) -> dict[str, tuple[Path, str]]:
    fixtures = Path(__file__).resolve().parents[2] / "fixtures" / "snapshots"
    return {version: (directory / version, capture_commit(
        fixtures / version, directory / version, f"Demo snapshot {version}",
    )) for version in ("v1", "v2")}


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="fl-git-snapshots-") as temporary:
        versions = prepare_versions(Path(temporary))
        if sys.argv[1:] == ["hashes"]:
            for _, sha in versions.values():
                print(sha)
        elif len(sys.argv) == 4 and sys.argv[1] == "publish":
            root = Path(sys.argv[2])
            manifest = []
            with open(sys.argv[3], newline="") as file:
                for project, snapshot, owner, version, expected in csv.reader(file):
                    source, sha = versions[version]
                    if sha != expected:
                        raise ValueError("Prepared commit does not match the fixture manifest")
                    publish_commit(root, int(project), int(snapshot), source, sha)
                    manifest.append(dict(project_id=int(project), snapshot_id=int(snapshot),
                        owner_id=int(owner), version=version, git_commit_sha=sha))
            print(json.dumps(manifest))
        else:
            raise SystemExit("Usage: git_snapshots.py hashes | publish GIT_STORAGE_DIR MANIFEST.csv")


if __name__ == "__main__":
    main()
