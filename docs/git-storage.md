# Local Git storage

Only the Rust API accesses repositories. Install Git on the API host; no Python,
SSH service, keys, Docker Git container, or setup script is required.

API startup creates `storage/git/projects/` at the repository root. Each project
uses a bare repository named `<project-id>.git`. Existing repositories remain
usable in place. Optional `GIT_STORAGE_DIR` in `.env` sets an absolute alternative
Git storage root; repositories live in its `projects/` subdirectory.
Demo snapshot archives default to `storage/snapshots/`.

After authenticating the caller and checking project permissions, handlers can
use `AppState.git`:

- `create_project_repository(project_id)` creates or reuses a bare repository.
- `publish_snapshot(project_id, snapshot_id, source, commit_sha)` imports a commit
  from an API-owned local Git repository and creates `refs/snapshots/<snapshot-id>`.
- `list_snapshot_refs(project_id)` reads the published snapshot references.

Git operations return `Result<_, GitError>`, distinguishing invalid input, invalid
output, I/O errors, timeouts, and failed commands. I/O errors retain their original
source; failed commands retain exit status and stderr for internal diagnostics.
Avoid exposing these diagnostics directly in HTTP responses.

Publication requires a full SHA-1 commit ID. Snapshot IDs contain ASCII letters,
digits, underscores, or hyphens and start with a letter or digit. Git atomically
checks that the reference does not exist, so concurrent publications cannot
replace it. There is no snapshot deletion or replacement method. Administrators
with filesystem access can still change repositories directly.

The source repository must remain available and unchanged during import. HTTP
uploads, constructing commits from uploaded files, database snapshot publication,
and deployment execution are separate work. Existing project creation still
creates only the database record; handlers must call the Git helpers explicitly.

Storage is excluded from version control. Restrict access to the API account and
back up storage privately. New directories use mode 0700 on Unix. Symlink storage
paths are rejected. Require any storage mount before starting the API.

For installations using the previous setup, stop and remove the old
`federated-learning-git` container. Repositories are retained in `storage/git/projects/`;
old `storage/git/ssh/` keys are no longer used. `GIT_SSH_PORT` is no longer used.
Existing demo archives have moved from the former storage location into
`storage/snapshots/`; their relative database storage keys remain unchanged.
An explicit `SNAPSHOT_STORAGE_DIR` override should point to the new location.

Run `cargo test` from `api/` to exercise local creation, publication, and retention
without a running Git service. Database integration tests remain opt-in.
