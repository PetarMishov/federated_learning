#!/usr/bin/env python3
"""Prepare persistent SSH identities and start a localhost-only Git container."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]
CONTAINER = "federated-learning-git"
IMAGE = "federated-learning-git:local"


def setting(name: str, default: str) -> str:
    if name in os.environ:
        return os.environ[name]
    env = ROOT / ".env"
    if env.exists():
        for line in env.read_text().splitlines():
            key, separator, value = line.partition("=")
            if separator and key.strip() == name:
                return value.strip().strip("\"").strip("'")
    return default


def private_directory(path: Path) -> None:
    for component in (path, *path.parents):
        if component.is_symlink():
            raise ValueError("Git storage directories must not contain symlinks")
    path.mkdir(parents=True, exist_ok=True, mode=0o700)
    path.chmod(0o700)


def write_private(path: Path, content: str) -> None:
    if path.is_symlink():
        raise ValueError("SSH configuration files must not be symlinks")
    path.write_text(content)
    path.chmod(0o600)


def identity(path: Path, comment: str) -> str:
    if path.is_symlink() or path.with_suffix(".pub").is_symlink():
        raise ValueError("SSH identities must not be symlinks")
    if not path.exists():
        subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(path), "-C", comment], check=True)
    path.chmod(0o600)
    # Derive the public key from the private key, including on repeated setup.
    public_key = subprocess.check_output(["ssh-keygen", "-y", "-f", str(path)], text=True).strip()
    write_private(path.with_suffix(".pub"), public_key + "\n")
    return public_key


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--prepare-only", action="store_true", help="Generate keys without starting Docker")
    parser.add_argument("--replace", action="store_true", help="Recreate this managed container while preserving its bind-mounted storage")
    args = parser.parse_args()
    for executable in ("git", "ssh", "ssh-keygen", "docker"):
        if shutil.which(executable) is None and not (executable == "docker" and args.prepare_only):
            raise SystemExit(f"Install {executable} before running setup")
    root = Path(setting("GIT_STORAGE_DIR", str(ROOT / "storage" / "git")))
    port = int(setting("GIT_SSH_PORT", "2222"))
    if not root.is_absolute() or any(character in str(root) for character in (",", "\n", "\r")):
        raise SystemExit("GIT_STORAGE_DIR must be an absolute path without commas or newlines")
    if not 1024 <= port <= 65535:
        raise SystemExit("GIT_SSH_PORT must be between 1024 and 65535")
    if os.getuid() == 0 or os.getgid() == 0:
        raise SystemExit("Run setup as the non-root account that runs the API")
    private_directory(root)
    root = root.resolve()
    for directory in (root / "projects", root / "ssh" / "client", root / "ssh" / "server"):
        private_directory(directory)
    client = root / "ssh" / "client"
    server = root / "ssh" / "server"
    client_key = identity(client / "id_ed25519", "federated-learning-api")
    host_key = identity(server / "ssh_host_ed25519", "federated-learning-git-host")
    write_private(server / "authorized_keys", "restrict " + client_key + "\n")
    write_private(client / "known_hosts", f"[127.0.0.1]:{port} {host_key}\n")
    print(f"Git SSH endpoint: git@127.0.0.1:{port}")
    print(f"Repositories: {root / 'projects'}")
    print(f"API identity: {client / 'id_ed25519'}")
    if args.prepare_only:
        return
    subprocess.run(["docker", "build", "--build-arg", f"SERVICE_UID={os.getuid()}",
                    "--build-arg", f"SERVICE_GID={os.getgid()}", "-t", IMAGE, str(ROOT / "git-server")], check=True)
    image = json.loads(subprocess.check_output(["docker", "image", "inspect", IMAGE], text=True))[0]
    # Store the runnable image's fingerprint on the container. Docker may remove
    # old build-provenance image IDs after a cached rebuild.
    image_content = hashlib.sha256(json.dumps(
        {"filesystem": image["RootFS"], "configuration": image["Config"]}, sort_keys=True,
    ).encode()).hexdigest()
    existing = subprocess.run(["docker", "container", "inspect", CONTAINER],
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    reuse = False
    if existing.returncode == 0:
        container = json.loads(existing.stdout)[0]
        expected_mounts = {"/repositories": str(root / "projects"), "/keys": str(server)}
        actual_mounts = {mount["Destination"]: mount["Source"] for mount in container["Mounts"] if mount["Type"] == "bind"}
        ports = container["HostConfig"]["PortBindings"].get("2222/tcp")
        labels = container["Config"].get("Labels") or {}
        if actual_mounts != expected_mounts or labels.get("federated-learning.service") != "git":
            raise SystemExit("Refusing to reuse or replace a container with unrelated storage or ownership")
        same_configuration = (ports == [{"HostIp": "127.0.0.1", "HostPort": str(port)}]
                              and labels.get("federated-learning.image-content") == image_content)
        if same_configuration:
            subprocess.run(["docker", "start", CONTAINER], check=True)
            reuse = True
        elif args.replace:
            subprocess.run(["docker", "stop", CONTAINER], check=True)
            subprocess.run(["docker", "rm", CONTAINER], check=True)
        else:
            raise SystemExit("Existing Git container needs an update. Rerun with --replace during a maintenance window; repositories and keys will be retained.")
    if not reuse:
        subprocess.run(["docker", "run", "-d", "--name", CONTAINER,
                        "--label", "federated-learning.service=git",
                        "--label", f"federated-learning.image-content={image_content}",
                        "--restart", "unless-stopped",
                        "--read-only", "--security-opt", "no-new-privileges:true",
                        "--tmpfs", "/run:rw,nosuid,nodev", "--tmpfs", "/tmp:rw,nosuid,nodev",
                        "-p", f"127.0.0.1:{port}:2222",
                        "--mount", f"type=bind,source={root / 'projects'},target=/repositories",
                        "--mount", f"type=bind,source={server},target=/keys,readonly", IMAGE], check=True)
    for _ in range(30):
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.5) as connection:
                if connection.recv(255).startswith(b"SSH-2.0-"):
                    print("Git server started. No public SSH port was published.")
                    return
        except OSError:
            pass
        time.sleep(0.25)
    raise SystemExit("Git SSH server did not become ready; inspect docker logs federated-learning-git")


if __name__ == "__main__":
    os.umask(0o077)
    main()
