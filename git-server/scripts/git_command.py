#!/usr/bin/python3
"""SSH forced command: Git transport only, within numeric project repositories."""
import os
from pathlib import Path
import re
import shlex
import sys


def git_arguments(command: str) -> list[str]:
    arguments = shlex.split(command)
    if len(arguments) != 2 or arguments[0] not in ("git-upload-pack", "git-receive-pack"):
        raise ValueError("Only Git fetch and push are allowed")
    if not re.fullmatch(r"/repositories/[1-9][0-9]*\.git", arguments[1]):
        raise ValueError("Invalid project repository")
    project = int(Path(arguments[1]).name.removesuffix(".git"))
    if project > 2_147_483_647:
        raise ValueError("Invalid project identifier")
    repository = Path(arguments[1])
    if repository.is_symlink() or repository.resolve() != repository:
        raise ValueError("Repository must not be a symlink")
    if not repository.is_dir() or not (repository / "HEAD").is_file():
        raise ValueError("Project repository does not exist")
    # Use image-owned hooks, never hooks from uploaded project content.
    return ["git", "-c", "core.hooksPath=/usr/local/libexec/git-hooks",
            "-c", "receive.denyDeletes=true", "-c", "receive.denyNonFastForwards=true",
            arguments[0].removeprefix("git-"), str(repository)]


def main() -> None:
    try:
        arguments = git_arguments(os.environ.get("SSH_ORIGINAL_COMMAND", ""))
    except ValueError as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(1)
    environment = {"PATH": "/usr/bin:/bin", "HOME": "/home/git",
                   "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": "/dev/null"}
    os.execve("/usr/bin/git", arguments, environment)


if __name__ == "__main__":
    main()
