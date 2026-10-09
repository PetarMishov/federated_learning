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
Uploads through this endpoint are recorded as `source: "local"`. Provider drafts
record source provenance through the import endpoints. Include project code only, excluding local datasets.

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

Create a GitLab personal access token with `read_api` and `read_repository`
scopes. `read_api` is required for branch discovery; older tokens with only
`read_user` and `read_repository` must be replaced to load branches.
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
`/connectors` to the API. The frontend Connectors page supports both providers;
project import uses the saved connection.

References: [GitLab token API](https://docs.gitlab.com/api/personal_access_tokens/)
and [read scopes](https://docs.gitlab.com/security/tokens/access_token_scopes/).

## Provider repositories

`GET /connectors/{provider}/repositories?page=1` accepts `github` or `gitlab` and
requires a local session bearer token. It returns a page of up to 100 repositories:

```json
{"repositories":[{"id":42,"full_name":"Lab/Training","web_url":"https://gitlab.com/Lab/Training"}],"has_more":false}
```

The API looks up only the caller's active, unexpired connection and decrypts its
credential with `CONNECTOR_TOKEN_KEY`. The existing `v1:` encrypted token format
is preserved; encryption and decryption are shared by the connectors. Credentials
are never returned to the browser. Requests only use GET and redirects are rejected.
GitHub discovery uses `/user/repos`; GitLab uses
`/api/v4/personal_access_tokens/self/associations` (GitLab 17.4+), which supports
read token scopes. Branch discovery requires `read_api` on GitLab. GitLab
requests include `min_access_level=20` (Reporter or higher) to limit discovery to
repository read access and avoid expensive unfiltered public associations.
Public projects without membership and roles below Reporter are excluded.
Provider
pagination URLs are constructed locally. A full final page may trigger one extra
request before `has_more` becomes false. Pages must be between 1 and 10000.

Missing, expired, or invalidated connections return `404`; rejected provider tokens
return `403`; provider/network errors return `502`. Local session failures return
`401`; absent connector configuration returns `503`; database/decryption failures
return `500`. Responses disable caching. GitHub discovery requires a saved GitHub
connection.

The frontend fetches pages on opening the selector, searches every loaded
repository name and namespace without case sensitivity, and clears repository
selection on provider changes. The repository choice is kept in the project form
alongside branch/commit inputs; Load project imports the selected exact commit as an editable draft.

References: [GitHub repository discovery](https://docs.github.com/en/rest/repos/repos#list-repositories-for-the-authenticated-user)
and [GitLab token associations](https://docs.gitlab.com/api/personal_access_tokens/#list-all-token-associations).

## GitHub token authorization

`POST /connectors/github/authorize` requires a local session bearer token and
JSON `{"token":"github_pat_..."}`. It verifies the account using `/user` and
repository discovery using `/user/repos`, then saves an encrypted connection
for the authenticated local user. Reconnecting replaces that user's GitHub
connection. Responses contain only provider, external account ID, and username.

Use a fine-grained personal access token for the selected repositories with
Contents read-only permission; classic tokens can use `repo` for private
repositories. Fine-grained permissions cannot be inferred from OAuth scope
headers: GitHub enforces access on each repository request. Authorization
verifies identity and discovery, not Contents access to every repository.
Returned OAuth scopes and token expiry are stored when GitHub provides them.
Missing expiry stays NULL; GitHub still rejects expired or revoked tokens.

`CONNECTOR_TOKEN_KEY` enables this connector using the same key as GitLab.
The frontend Connectors page supports masked token entry for both providers.
Status codes follow GitLab: `401` for invalid local sessions, `400` for rejected
provider tokens or permissions, `502` for provider outages/rate limits, and
`503` when encryption is not configured. Successful responses use `no-store`.

Reference: [GitHub personal access tokens](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens).

`GITHUB_BASE_URL` configures the API base used by both GitHub authorization and
repository discovery. It defaults to `https://api.github.com`. For a private
GitHub Enterprise Server, set `GITHUB_BASE_URL=https://github.example.com/api/v3`.
A trailing slash is optional. HTTPS is required except for loopback development
servers; credentials, query strings, fragments, and unrelated paths are rejected.
Restart the API after changing this setting.

Reference: [GitHub Enterprise REST API base URL](https://docs.github.com/en/enterprise-server@3.19/rest/using-the-rest-api/getting-started-with-the-rest-api).

## Branch selection

`GET /connectors/{provider}/branches?repository=...&page=1` lists branches and
current head commits using the authenticated user's saved provider token.
For GitHub, `repository` is `owner/repo`; for GitLab, it is the numeric project ID.
The response is `{"branches":[{"name":"main","commit_sha":"..."}],"has_more":false}`.
Pages contain up to 100 branches. Local session validation, encrypted credential
lookup, redirect rejection, API base URLs, and no-store responses match repository
discovery. GitHub Enterprise paths retain `/api/v3/`.

GitHub tokens need Contents read permission. GitLab tokens need `read_api` for
`/projects/:id/repository/branches`; `read_repository` alone does not grant it.
Repository access or token permission failures return `403`; provider failures
return `502`. The project form offers searchable branch selection and defaults
the editable commit SHA to the selected branch's head. Changing repository,
provider, or branch resets the commit selection. Historical SHA selection is
used by Load project; no deployment action is added.

References: [GitLab branches API](https://docs.gitlab.com/api/branches/),
[GitHub branches API](https://docs.github.com/en/rest/branches/branches#list-branches).


## Project import drafts

`GET /projects/{id}/import-limits` returns `max_bytes` and `max_files`. Configure
`PROJECT_IMPORT_MAX_BYTES` (default 104857600) and `PROJECT_IMPORT_MAX_FILES`
(default 10000) in the root `.env`; both must be positive. Restart after changes.
Limits count actual file contents and apply again when publishing an edited draft.
Snapshot edit patches retain the existing 64 MiB JSON request limit.

`POST /projects/{id}/imports` starts a draft and returns `202` with its UUID/status:

```json
{"source":"local"}
```

```json
{"source":"github","repository":"owner/repo","branch":"main","commit_sha":"FULL_40_CHARACTER_SHA"}
```

For GitLab use its numeric project ID as `repository`. Provider credentials are
read from the caller's encrypted connection. GitLab needs `read_api` and
`read_repository`; GitHub needs Contents read. The configured private provider
server is also used for imports. No recursive submodule/LFS fetching occurs.

For local imports, `POST /projects/{id}/imports/{uuid}/files` streams multipart
file fields. Field names are URI-encoded relative paths; files are stored under
numbered private staging names, not caller filesystem paths. Byte/file limits,
path validation and duplicate checks run during upload. The browser applies
`.gitignore` rules; the API independently enforces path and capacity limits.
Reverse proxies must permit the configured content size plus multipart overhead
and a suitable upload duration.

Poll `GET /projects/{id}/imports/{uuid}`. Status includes `phase`
(`uploading`, `downloading`, `preparing`, `ready`, `failed`, `cancelled`), nullable
`progress_percent`, actual `bytes`/`files`, and nullable `error`. Percentages
measure transfer progress; preparing has no fabricated percentage. Use `DELETE`
on that URL to cancel/discard. Deleting a draft while publication is running
returns `409`. Provider downloads have a five-minute deadline and a temporary
Git-storage ceiling of twice the contents limit plus 16 MiB.

Ready drafts expose `GET .../{uuid}/tree?path=` and `GET .../{uuid}/file?path=`
with the existing tree/file formats and preview limits. `POST .../{uuid}/snapshot`
accepts the existing `files`/`operations` patch, without `base_snapshot_id`.
An empty patch saves the complete imported tree. The `201` response is snapshot
metadata, including provider provenance. Saving removes the temporary draft.

All draft operations require a valid session, project editing permission, and
matching user/project ownership. Discard still permits owner cleanup after edit
permission is revoked. Limits and draft read responses use `no-store`. Staging
expires after 30 idle minutes; at most two drafts per user and eight total are
retained. Jobs are temporary and are lost when the API restarts. After a crash, startup and periodic cleanup remove abandoned private `.imports-*`
directories once they are at least one minute old. Filesystem leases protect directories owned by another live API process.

The [import contract](../docs/project-import.md) and
[storage decision](../docs/adr/0003-stage-project-imports-before-saving-snapshots.md)
explain the publication boundary and browser limitations.
