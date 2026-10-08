# Private Git SSH server

This dedicated Docker service stores bare repositories under
`storage/git/projects/` and publishes SSH only on **127.0.0.1:2222**. It does not
change the machine's normal SSH configuration or authorized keys.

## Setup and operation

Run from the repository root as the same non-root user that runs the API:

```bash
python3 git-server/scripts/setup_server.py
```

The script generates persistent Ed25519 identities, builds the local image, and
starts `federated-learning-git`. Repeated setup reuses private keys. The host needs
Git, OpenSSH client tools, Python 3, Docker, and Docker access; OpenSSH server is
installed inside the image. The API also needs the host Git and SSH executables.

Optional root `.env` settings, read by both setup and API startup:

```dotenv
GIT_STORAGE_DIR=/absolute/path/on/raid/storage/git
GIT_SSH_PORT=2222
```

Defaults are this checkout's `storage/git/` and port 2222. The storage directory
must be a real absolute path without symlink ancestors, commas, or newlines.
Ports must be 1024–65535. For RAID storage, require that mount before starting
Docker or the API to prevent fallback writes to the underlying system disk.

```bash
docker logs federated-learning-git
docker port federated-learning-git
docker stop federated-learning-git
docker start federated-learning-git
```

The container restarts after host/Docker restart unless explicitly stopped.
If setup reports an image or port mismatch, rerun with `--replace` during a
maintenance window. This recreates only this managed container and retains its
bind-mounted repositories and keys. Unrelated containers or storage paths are
never replaced automatically. Changing the storage path requires explicit
container removal after you have arranged the data move. Do not delete `storage/`.
`--prepare-only` generates/verifies keys without building or starting Docker.

## Keys and AppState

Two different key pairs serve different roles:

```text
storage/git/
  projects/<project-id>.git/
  ssh/
    client/id_ed25519          API authentication private key (0600)
    client/id_ed25519.pub
    client/known_hosts        Pinned server public host key (0600)
    server/ssh_host_ed25519    Server identity private key (0600)
    server/ssh_host_ed25519.pub
    server/authorized_keys    Only the API's public key
```

`AppState.git` is a clonable `GitClient`. Its `ssh_identity` contains
`private_key_path` and `known_hosts_path`; private bytes stay on disk instead of
being copied into every state clone. API startup fails with a setup instruction
if identity files are absent, have unsafe permissions, or use symlinks. It does
not generate keys at startup; Git need only be running when an operation is used.

The API private key is never mounted into the server. The server receives its
own host key and the API public key. Storage is ignored by the application's Git
repository. Back up repositories and keys privately; RAID cannot restore deleted
files. Do not put private keys in `.env`, HTTP responses, logs, snapshots, or
frontend assets.

## Use from API handlers

After authenticating the caller and checking database project permissions:

```rust
let repository = state.git.create_project_repository(project_id).await;
let snapshots = state.git.list_snapshot_refs(project_id).await;
```

Both return `std::io::Result`. Map failures to HTTP errors without exposing raw
command stderr. Other helpers are `remote_url(project_id)` and
`repository_root()`. A project's URL is:

```text
ssh://git@127.0.0.1:2222/repositories/<project-id>.git
```

Repository initialization is local because the API and server share a directory,
and is serialized per project within the API process. Listing snapshot refs uses
the actual SSH transport. Positive numeric IDs prevent client-supplied filesystem
paths and command options from becoming repository URLs.

The service identity accesses all project repositories, so user authorization
remains the API's responsibility. Existing project creation does not automatically
initialize a Git repository. HTTP snapshot uploads, size limits, database
publication, and container execution remain separate work. Existing demo archives
and database storage references are unchanged.

## Access restrictions

Only public-key authentication is enabled. Forwarding, PTYs, password login,
interactive shells, SFTP, SCP, and arbitrary commands are unavailable. The forced
command accepts only `git-upload-pack` and `git-receive-pack` for numeric project
repositories under `/repositories/`, without evaluating shell commands. Server
hooks come from the image, not uploaded repositories.

The API SSH wrapper uses strict pinned host-key verification, its dedicated key,
no SSH agent or user SSH configuration, and noninteractive timeouts. Unknown or
changed host keys fail. Docker publishes only a localhost port; the HTTP API's
listener is separate. Running the API in another container or on another machine
would require an explicit networking change.

The server rejects ref deletion and non-fast-forward branch updates. The
pre-receive hook makes `refs/snapshots/<snapshot-id>` append-only and requires
snapshot refs to point to commits. Direct filesystem or Docker administrator
access can bypass these SSH guarantees.

## Validation

```bash
python3 git-server/tests/test_server.py
cd api
cargo test
cargo test local_ssh_server_supports_snapshots_and_rejects_overwrites -- --ignored
```

The live test creates temporary repositories, transfers source files over SSH,
checks retention, rejects overwrite/deletion, rejects incorrect keys and
untrusted host keys, and denies shell commands. It cleans up its own fixtures.
Set the same `GIT_STORAGE_DIR` and `GIT_SSH_PORT` in the test environment if you
changed their defaults.

References: [Git's SSH server model](https://git-scm.com/book/en/v2/Git-on-the-Server-Setting-Up-the-Server),
[OpenSSH server restrictions](https://man.openbsd.org/sshd_config), and
[client identity and host-key options](https://man.openbsd.org/ssh_config).
