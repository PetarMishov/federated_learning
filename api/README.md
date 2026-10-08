# API endpoints

API startup prepares local Git storage in `storage/git/projects/` through
`AppState.git`. Only the Git executable is required; see
[local Git storage](../docs/git-storage.md).

## Project members

`GET /projects/{proj_id}/members` requires a valid bearer token and returns:

```json
{"members": [{"id": 1, "username": "alice", "role_id": 2, "role_name": "Coordinator"}]}
```

Members are the organization owner plus current organization members whose roles
have at least one permission on this project. Users appear once even when their
role grants multiple permissions, sorted by username and ID. Role fields may be
null, including for an owner with no assigned role. This list is separate from
deployment participation.

Any current member of the project's organization can read the list. A caller
outside that organization, or a nonexistent project, receives an empty list,
matching the existing list endpoints. Invalid or revoked tokens return `401`;
database failures return `500`.

The frontend loads this list when the project page's Members drawer opens,
refreshes it on reopening, and offers retry on errors. The development proxy
forwards `/projects/{id}/members` to the API.

## Creation

Both endpoints require `Authorization: Bearer <token>` and
`Content-Type: application/json`. Ownership and creator IDs come from the verified
session; request bodies contain only a name.

| Endpoint | Access | Result |
| --- | --- | --- |
| `POST /organizations` | Any authenticated user | Organization owned by the caller, with owner membership |
| `POST /organizations/{org_id}/projects` | Organization owner | Project belonging to that organization |

For either endpoint, submit:

```json
{"name": "Research lab"}
```

Names are trimmed and must contain 1–255 characters, with no internal control
characters. Additional fields are rejected. Names need not be unique under the
current database schema.

Successful creation returns `201 Created` and the created object:

```json
{"id": 1, "name": "Research lab", "owner_user_id": 1}
```

```json
{"id": 1, "org_id": 1, "created_by_user_id": 1, "name": "Training"}
```

The organization and its owner's membership are inserted in one transaction.
Project creation checks and locks current ownership and membership while
inserting. New projects use `/data/input` and `/data/output` as container mount
destinations and `.` as the build context, matching existing demo defaults.
Project creation also initializes its empty bare Git repository before committing
the project row. Creation does not publish a snapshot or start a deployment.

Missing, invalid, or revoked tokens return `401`. Invalid names return `400`;
malformed JSON returns `400`, and missing, unknown, or incorrectly typed fields
return `422`. Missing JSON content type returns `415`. Project creation returns
`403` when the caller is not the organization owner or the organization does not
exist. Database failures return `500` without exposing database details.

The current permission catalog has no organization-level project-creation grant.
Project creation therefore uses an owner-only policy; role-management permission
does not grant it.

Run regular tests from `api/` with `cargo test`. The creation integration test
uses its own temporary schema, leaves existing application data untouched, and
requires a database account that can create schemas:

```bash
TEST_DATABASE_URL='<postgres connection URL>' cargo test \
  creation_endpoints_persist_membership_and_enforce_project_ownership -- --ignored
```

## Snapshot metadata

`GET /projects/{proj_id}/snapshots/{snapshot_id}` requires a valid bearer token.
The organization owner or a current organization member with a permission grant
for this project can read the snapshot. Organization membership alone is insufficient.
The snapshot must belong to the project named in the URL.

Successful requests return `200` with:

```json
{
  "id": 7,
  "project_id": 42,
  "created_by_user_id": 1,
  "source": "local",
  "source_branch": null,
  "source_commit_sha": null,
  "git_commit_sha": "0123456789abcdef0123456789abcdef01234567",
  "created_at": 1790812800000.0
}
```

`created_at` is Unix time in milliseconds, matching deployment responses.
External branch and commit provenance may be null and is separate from the stored
Git commit ID. This endpoint reads metadata from the database; it does not read
file contents or verify Git storage. Repository URLs and provider credentials are
not included in the response.

Missing, invalid, or revoked credentials return `401`. Missing snapshots,
wrong-project IDs, and callers without project access all return the same `404`.
Database failures return `500` without exposing internal errors.

On the frontend project page, the newest saved snapshot is selected automatically,
including projects with no deployments. Use **Saved snapshots** to select any
loaded version; **Load older snapshots** fetches more when needed. **Close** clears
the viewer and leaves the selection empty until you choose another snapshot.
Deployment loading does not change snapshot selection. A deployment's **View snapshot**
button remains a shortcut to its specific version. Compact details beside the
**Saved snapshots** selector show the saved date, source, and stored commit ID,
plus original branch/commit when available. Full commit IDs appear on hover. It supports
retry and close and returns to login on an expired session.

## Snapshot list

`GET /projects/{proj_id}/snapshots?limit=50&offset=0` requires the same current
project access as snapshot metadata. It returns `{"snapshots": [...], "has_more": false}`,
using the metadata fields above. Results are sorted by `created_at DESC, id DESC`,
so the first item is the newest saved version even if an older snapshot has a
higher ID or is used by a newer deployment. `limit` defaults to 50 and must be
between 1 and 100; `offset` defaults to 0 and must be nonnegative.
An authorized project with no snapshots returns an empty list. Missing or
inaccessible projects return `404`; invalid or revoked sessions return `401`.

## Snapshot files

Both endpoints require the same authentication and project access as metadata.
Reads use the saved `git_commit_sha`; they never follow a moving branch or read
uploaded paths from the host filesystem.

`GET /projects/{proj_id}/snapshots/{snapshot_id}/tree?path=src` lists immediate
children of a directory. Omit `path` or use an empty value for the root:

```json
{"path":"src","entries":[{"name":"train.py","path":"src/train.py","kind":"file"}]}
```

Entry kinds are `directory`, `file`, `symlink`, and `submodule`. Directories appear
first, followed by other entries, sorted by name. Symlinks/submodules are listed
but never followed by the file viewer.

`GET /projects/{proj_id}/snapshots/{snapshot_id}/file?path=src/train.py` returns:

```json
{"path":"src/train.py","content":"print('snapshot')\n"}
```

Paths must be relative, with no empty components, `.`/`..`, backslashes, or NULs.
Invalid paths return `400`; nonexistent paths return `404`. File viewing accepts
regular UTF-8 text files without NUL bytes up to 8 MiB (`MAX_PREVIEW_BYTES`). This
limit applies only to viewing, not Git storage or the total snapshot size.
Unsupported entries or binary files return `415`; oversized files return `413`
with JSON containing `error: "preview_too_large"`, `size_bytes`, and
`max_preview_bytes`. The editor shows the file size and preview limit, with a
disabled **Download file** placeholder until file downloads are implemented.
Storage failures return
`500` without exposing Git stderr or host paths.

The Files pane loads the selected snapshot's root directory. Click folders to
browse, use **Up** to return to a parent directory, and click a file to show its
contents in the read-only editor. Contents are rendered as escaped text. Loading
and retry states are independent for folders and files; changing snapshots or
projects cancels outstanding reads and clears previous contents.

## Snapshot placeholders

These routes are registered and return `501 Not Implemented`. Each handler has its
own `.rs` file in `src/routers/projects/`. Authentication, request/response types,
permission checks, uploads, and archive generation remain TODOs.

| Endpoint | Planned purpose |
| --- | --- |
| `POST /projects/{proj_id}/snapshots` | Save a complete file tree as an immutable Git snapshot |
| `GET /projects/{proj_id}/snapshots/{snapshot_id}/archive` | Generate a download of the complete saved file tree |

The database stores `git_commit_sha`; files are retained in the project's repository
at `refs/snapshots/<snapshot-id>`. See [Git storage](../docs/git-storage.md).
