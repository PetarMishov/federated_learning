# Local Git snapshot storage

The Rust API stores one bare Git repository per project at
`storage/git/projects/<project-id>.git`. Set an absolute `GIT_STORAGE_DIR` in
`.env` to override the Git root; repositories remain in its `projects/` directory.
Only Git is needed at runtime. There is no SSH service or Git hosting container.

A snapshot is a complete immutable file tree represented by a Git commit.
The database stores `git_commit_sha` and metadata; the project repository retains
that commit permanently at `refs/snapshots/<snapshot-id>`. Multiple snapshots
share the project's repository. `source_commit_sha` is optional provenance from
an external repository, not the identifier of the locally stored snapshot.
Snapshots do not require separate extracted directories or permanent archives.

Project creation inserts the project within a database transaction, creates its
empty repository, and then commits. Failed commits are checked after the original
transaction finishes. Cleanup removes storage only when the project is confirmed
absent. Unknown outcomes retain storage for later reconciliation.

After authentication and project authorization, use `AppState.git`:

- `create_new_project_repository(project_id)` creates storage exclusively for a new project.
- `create_project_repository(project_id)` initializes or reuses a bare repository.
- `publish_snapshot(project_id, snapshot_id, source, commit_sha)` imports an
  API-owned source commit and atomically creates its permanent snapshot reference.
- `list_snapshot_refs(project_id)` reads retained snapshot references.

References cannot be replaced through publication. Commits use full SHA-1 IDs.
Import the commit and retain its reference before committing snapshot metadata.
A failed database transaction may leave an unlisted reference; reconcile that
later without deleting published snapshots or commits used by deployments.
Database metadata and Git storage must be backed up together.

Snapshot HTTP routes currently return `501 Not Implemented`. Upload parsing,
commit construction, metadata publication, tree/file reading, and archive download
are future implementations. Downloads should generate archives from the saved
commit on demand. The API must resolve snapshots through their project and check
current permissions before reading files; it must not serve storage as public assets.

## Demo data

`./db/scripts/populate_db.sh` prepares deterministic commits from tracked fixtures,
imports them into the same project repositories used by the API, and publishes
`refs/snapshots/<snapshot-id>` before inserting snapshot/deployment metadata.
Population can be rerun without replacing published refs or duplicating fixtures.
Python is required only by these development scripts.

Storage is excluded from version control. Restrict access to the API account and
back it up privately. Directories use mode 0700 on Unix; symlink storage paths are
rejected. Require the storage mount before starting the API.

Run `cargo test --manifest-path api/Cargo.toml` for runtime Git checks and
`python3 db/tests/test_git_snapshots.py` for demo storage checks. Database integration
tests use temporary schemas; see [the database README](../db/README.md).
