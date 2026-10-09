# Project loading and editable drafts

Implemented from the confirmed interview decisions.

## Agreed behavior

- Load project creates an editable draft, rather than a permanent snapshot.
- The user can inspect/edit the loaded files and explicitly save them as a snapshot,
  including an imported draft with no further edits.
- Projects up to 100 MiB should load comfortably without stalling the interface.
  This is also the default maximum total file-content size, configurable through
  an API environment variable. Transport/base64/compression overhead does not
  count as project file contents. The recommended default file-count ceiling is
  10,000 files.
- Show a compact completion/progress bar on the right side of Saved snapshots.
- Local Folder imports files from the visitor's computer, including when the API
  is hosted remotely. The browser folder chooser provides the selected files.
- The local folder display becomes read-only; no manually typed filesystem path
  is used to load files.
- Local imports honor `.gitignore` and exclude `.git`. Provider imports use files
  recorded at the selected commit.
- Drafts last for the current visit. Warn before discarding edits on leaving;
  refreshing loses the unsaved draft. Recovery across visits is deferred.
- Ask before replacing unsaved edits. A successfully loaded new project replaces
  the previous draft; cancellation or failure preserves the existing draft.

## Constraints considered

- Loading needs temporary draft storage, separate from permanent snapshot publication.
- The existing editor browses exact saved commits; draft browsing must preserve lazy access.
- Complete snapshot uploads use JSON/base64 and are limited to 64 MiB per request
  and 10,000 files. This transport cannot handle the agreed 100 MB target as-is.
- Directory/file browsing is lazy; text previews have an independent 8 MiB limit.
- Snapshot saves preserve untouched Git objects and retain immutable snapshot refs.
- Browser file selection does not expose an absolute filesystem path. Typing a
  path alone does not grant access to files on the visitor's computer.

## Resolved loading details

- Display the selected local folder name in the read-only field; the browser does
  not provide its absolute path.
- Use a measured percentage when transfer totals are known. When totals are
  unknown or processing is underway, show an animated bar with a short phase
  label. Never present a guessed completion percentage as measured progress.
- Import only the selected repository at the selected commit, without recursively
  fetching other repositories. Reject provider imports containing submodules
  with an actionable explanation: the chosen repository must contain its code.
- Preserve symbolic links as links rather than following targets; keep regular
  binary files even if they cannot be text-previewed.
- Saving an imported draft records its source and exact original commit when
  available. It creates a permanent snapshot only on explicit Save snapshot.

## Implementation direction

Use bounded transfer and temporary staging rather than sending a complete
100 MiB project as one JSON/base64 payload. Keep the page responsive, and browse
loaded directories and file contents on demand. Existing snapshots remain
available while a replacement is loading, and failures must not publish partial
snapshots or discard the prior draft.

Enforce the configured byte/file limits in the API; expose the configured limits
to the frontend for early local-folder validation. Limits apply after local
ignore filtering and to actual file contents, including decompressed provider
contents. Keep provider credentials scoped to the requesting user and enforce
project editing permissions for importing and saving.

An import draft is temporary and scoped to the current user/project. Release its
staging resources after discard, successful save, cancellation, or expiry. No
cross-visit recovery is promised. Existing snapshot references stay immutable.


No deployment action is part of this loading feature.

## Delivered behavior and limits

The default capacity is controlled by `PROJECT_IMPORT_MAX_BYTES=104857600` and
`PROJECT_IMPORT_MAX_FILES=10000`. Local multipart uploads stream to private disk
staging; ready drafts use lazy Git tree/file APIs. Provider imports use an exact
SHA, authenticated shallow Git fetch, no checkout, no submodule recursion, and
no LFS object downloads. A parentless commit retains the complete versioned tree
without importing history into permanent snapshot storage. Saving records the
provider/branch/original SHA independently from the stored commit.

Only Git-versioned provider blobs are loaded, including LFS pointer files as
recorded. Binary files remain in snapshots. Local browser selection supplies
regular file contents and cannot preserve symlink/executable metadata or expose
absolute paths; provider Git imports preserve modes and symlinks.

Staging lasts until explicit discard, cancellation, save, or 30 idle minutes.
The API permits two drafts per user/eight total and cancels provider transfers
after five minutes or excessive temporary storage. Server restarts discard draft
metadata; startup/periodic cleanup removes crash-left directories older than one minute,
while filesystem leases preserve live instances.

Verification includes frontend upload/cancellation/replacement/save tests,
Git tree/mode/submodule checks, an authenticated Git-over-HTTP shallow-fetch test,
a 100 MiB disk-staging/save test, and isolated PostgreSQL import/publication tests.
Transfer duration depends on bandwidth; the tests do not promise a fixed load
latency for every provider/server/network.

See [API details](../api/README.md#project-import-drafts) and the
[decision record](adr/0003-stage-project-imports-before-saving-snapshots.md).
