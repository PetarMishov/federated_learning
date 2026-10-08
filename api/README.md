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
loaded version; **Load older snapshots** fetches more when needed. The selector
always keeps a saved version selected when snapshots are available.
Deployment loading does not change snapshot selection. A deployment's **View snapshot**
button remains a shortcut to its specific version. Compact details beside the
**Saved snapshots** selector show the saved date, source, and stored commit ID,
plus original branch/commit when available. Full commit IDs appear on hover. It supports
retry and returns to login on an expired session.

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
Storage failures return `500` without exposing Git stderr or host paths.

The Files pane shows an expandable tree. Click folder arrows to expand or collapse
children; parent folders remain visible. Click files to edit their text. Right-click
an entry (or use Shift+F10) for **New file**, **New folder**, **Rename**, and **Delete**.
Right-click the background to create an entry at the root. Drag files or folders
onto folders to move them, or onto the background to move them to the root.
Names cannot overwrite other entries, and folders cannot move into themselves.
All changes remain drafts until saved. Arrow keys navigate the tree. Folder/file
reads have independent retry states and are cancelled when changing snapshots or
projects. Drafts survive switching snapshots within the project.

## Save a snapshot

`POST /projects/{proj_id}/snapshots` requires a Bearer token and current
`edit_project` permission, including the organization owner. Send the complete
file tree as JSON:

```json
{
  "files": [
    {"path": "train.py", "content": "print('saved')\n"},
    {"path": "src/run.sh", "content": "#!/bin/sh\necho ready\n", "executable": true},
    {"path": "assets/example.bin", "content_base64": "AP8B"}
  ]
}
```

To edit an existing snapshot, include `base_snapshot_id` and send only changed files:

```json
{"base_snapshot_id": 4, "files": [{"path": "train.py", "content": "print('edited')\n"}]}
```

The base must belong to this project. Existing regular files can be replaced and new files can be added, including in
new directories. Files cannot replace directories, symlinks or submodules, and
parent paths cannot traverse files, symlinks or submodules.
Untouched files (including binary files, large files, symlinks and submodules) and
existing executable modes are preserved. The base snapshot remains immutable.
Without `base_snapshot_id`, the request represents the complete replacement tree.

Optional `operations` are applied in order to the base tree before uploading file
contents. Rename and drag moves use `move`; deleting a directory removes its entire
subtree. No file downloads are needed, so binary bytes, symlinks, submodules, and
executable modes survive moves. Sources must exist and destinations must be vacant.
Tree operations require `base_snapshot_id` and are limited to 10000 per request.

```json
{
  "base_snapshot_id": 4,
  "operations": [
    {"kind": "move", "path": "src", "to": "lib"},
    {"kind": "delete", "path": "obsolete.py"}
  ],
  "files": [{"path": "lib/train.py", "content": "print('edited')\n"}]
}
```

Each file must specify exactly one of `content` (UTF-8 text) or
`content_base64` (standard padded base64 for arbitrary bytes). `executable`
defaults to `false`. Without a base, omitted files are absent from the new snapshot
and an empty `files` array saves an empty Git tree. With a base, omitted files remain
unless deleted by an operation. Earlier snapshots always remain intact.
Uploads are recorded as `source: "local"`; provider imports and source provenance
are separate future work. Include project code only, excluding local datasets.

Paths must be unique and relative, with no empty, `.` or `..` components,
backslashes, colons, control characters, or `.git` components (case-insensitive,
including trailing dots/spaces). A path cannot be both a file and a directory.
Paths are limited to 4096 UTF-8 bytes and components to 255 bytes. Unknown JSON
fields are rejected. Files are regular blobs; symlinks and submodules are not
created by uploads. Git filters, line-ending conversion, and ignore rules do not
alter or omit uploaded bytes.

The JSON request body is limited to 64 MiB, including JSON/base64 overhead, and
at most 10000 files. These upload limits are independent of the 8 MiB text
preview limit: larger files can be saved but cannot be previewed in full.

Successful saves return `201 Created` with the same snapshot metadata fields as
`GET /projects/{proj_id}/snapshots/{snapshot_id}`. The API captures the commit,
rechecks/locks editing authorization in a transaction, inserts metadata, and
creates `refs/snapshots/<snapshot-id>` before committing. Publication is serialized
per project, including across API processes. Uncertain database commit outcomes
are reconciled before returning success; unresolved/failed commits retain Git
data for later reconciliation without removing earlier snapshots.

Missing/invalid/revoked credentials return `401`; unavailable projects or missing
editing permission return `404`; invalid file paths/content return `400`;
invalid JSON structure/unknown fields return `422` (malformed JSON returns `400`);
upload limits return `413`; storage/database failures return a generic `500`.
Saving a snapshot does not create or replace deployments. The frontend enables
**Save as new snapshot** for added/edited files, moves, renames, or deletions.
The Files window can create draft files and folders in the current directory.
New files open immediately and enable saving even when empty. Empty folders remain
in the draft until they contain a file, because Git does not store empty folders.
Successful saves select the new snapshot and reopen the edited file; failed saves
keep drafts for retry. Import controls remain placeholders.

## Snapshot placeholders

These routes are registered and return `501 Not Implemented`. Each handler has its
own `.rs` file in `src/routers/projects/`. Authentication, request/response types,
permission checks and archive generation remain TODOs for archive downloads.

| Endpoint | Planned purpose |
| --- | --- |
| `GET /projects/{proj_id}/snapshots/{snapshot_id}/archive` | Generate a download of the complete saved file tree |

The database stores `git_commit_sha`; files are retained in the project's repository
at `refs/snapshots/<snapshot-id>`. See [Git storage](../docs/git-storage.md).

## GitLab token authorization

`POST /connectors/gitlab/authorize` requires a local session bearer token and JSON:

```json
{"token": "glpat-your-personal-access-token"}
```

Create a GitLab personal access token with `read_user` and `read_repository`
scopes. `read_api` can replace `read_user` if repository discovery is needed.
No write permission is required. Existing broader scopes that provide the same
read access are accepted; this connector makes only GET requests to GitLab.

The handler verifies token metadata with `GET /api/v4/personal_access_tokens/self`
and identity with `GET /api/v4/user`. It rejects inactive/revoked tokens or missing
read permissions, then inserts or updates only the authenticated local user's
GitLab connection. GitLab remains responsible for enforcing access to individual
repositories on subsequent requests.

Successful requests return `200` with public identity fields:

```json
{"provider":"gitlab","external_account_id":"42","external_username":"alice"}
```

Set `CONNECTOR_TOKEN_KEY` to a base64-encoded random 32-byte key, generated with
`openssl rand -base64 32`. The token is encrypted with AES-256-GCM, including a
random nonce and authenticated local-user/provider binding. Keep the key outside
the database and preserve it across restarts. Hashing would prevent recovering
the original token needed for later GitLab requests. Responses never expose it.

`GITLAB_BASE_URL` defaults to `https://gitlab.com`; a self-managed instance must
use an HTTPS origin, without a path, query, or credentials. Loopback HTTP is
allowed for development. Without an encryption key the connector returns `503`.
There is no OAuth application registration, callback, cookie, or refresh token.

Invalid local sessions return `401`; invalid token input, rejected GitLab tokens,
or insufficient read permissions return `400`; GitLab outages return `502`;
database/internal failures return `500`. Unknown request fields return `422`.
Reconnects replace only the caller's token and clear its invalidation timestamp.
Provider token expiry is stored as midnight UTC on GitLab's expiry date (or NULL
for a token without an expiry). Tokens must be replaced manually after expiry.

Route registration lives in `src/routers/connectors/mod.rs`, matching the other
routers. The handler, provider checks, and database persistence live in their
respective router, connector, and database modules. The development proxy forwards
`/connectors` to the API. Repository import and the frontend Connectors page remain
separate work.

References: [GitLab token API](https://docs.gitlab.com/api/personal_access_tokens/)
and [read scopes](https://docs.gitlab.com/security/tokens/access_token_scopes/).
