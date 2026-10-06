#!/usr/bin/env python3
"""Create deterministic, read-only demo archives; print their SHA-256 values."""
import hashlib
import io
import os
from pathlib import Path
import re
import sys
import tarfile
import tempfile


PROJECTS = [
    ("Central Hospital", "Patient risk prediction"),
    ("Central Hospital", "Medical image classification"),
    ("Research Lab", "Federated learning benchmark"),
    ("Research Lab", "Privacy-preserving model evaluation"),
    ("Research Lab", "Training algorithm comparison"),
    ("Medical Network", "Cross-hospital outcome prediction"),
]


def archive_bytes(version: str) -> bytes:
    fixture = Path(__file__).resolve().parents[2] / "fixtures" / "snapshots" / version
    output = io.BytesIO()
    with tarfile.open(fileobj=output, mode="w", format=tarfile.USTAR_FORMAT) as archive:
        for path in sorted(fixture.rglob("*")):
            if path.is_symlink():
                raise ValueError(f"Snapshot fixture contains a symlink: {path}")
            if not path.is_file():
                continue
            data = path.read_bytes()
            member = tarfile.TarInfo(path.relative_to(fixture).as_posix())
            member.size = len(data)
            member.mode = 0o444
            member.mtime = 0
            archive.addfile(member, io.BytesIO(data))
    return output.getvalue()


def write_once(root: Path, relative: Path, data: bytes) -> None:
    destination = root / relative
    # Check every path component; artifact keys must not escape through symlinks.
    for path in (destination, *destination.parents):
        if path.is_symlink():
            raise ValueError(f"Snapshot storage cannot contain symlinks: {path}")
    root.mkdir(parents=True, exist_ok=True, mode=0o700)
    destination.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    if destination.exists():
        if destination.read_bytes() != data:
            raise ValueError(f"Refusing to overwrite an existing snapshot: {destination}")
        return
    with tempfile.NamedTemporaryFile(dir=destination.parent, delete=False) as temporary:
        temp_path = Path(temporary.name)
        temporary.write(data)
        temporary.flush()
        os.fsync(temporary.fileno())
    try:
        temp_path.chmod(0o400)
        # Hard-link publication never replaces another process's snapshot.
        try:
            os.link(temp_path, destination)
        except FileExistsError:
            if destination.is_symlink() or destination.read_bytes() != data:
                raise ValueError(f"Snapshot conflict: {destination}")
    finally:
        temp_path.unlink(missing_ok=True)


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("Usage: snapshots.py SNAPSHOT_STORAGE_DIR")
    root = Path(sys.argv[1]).absolute()
    for version in ("v1", "v2"):
        data = archive_bytes(version)
        for organization, project in PROJECTS:
            if version == "v2" and project != "Patient risk prediction":
                continue
            slug = lambda name: re.sub(r"[^a-z0-9]+", "-", name.lower())
            relative = Path("demo") / slug(organization) / slug(project) / f"{version}.tar"
            write_once(root, relative, data)
        print(hashlib.sha256(data).hexdigest())


if __name__ == "__main__":
    main()
