# Stage project imports before saving snapshots

Load project creates a temporary editable draft owned by the authenticated user
and scoped to one project. Save snapshot is the publication boundary. Loading,
failed imports, and cancellations do not create snapshot rows or permanent refs.
An unchanged imported draft can be saved.

Stream browser-selected files as multipart data into numbered private staging
files. Apply local `.gitignore` rules before transfer and validate paths, content
size, and file count on the API. Store imported contents as Git objects and browse
files lazily. This avoids buffering or resending a complete 100 MiB project as
JSON/base64. Browser directory selection supplies file contents, but cannot supply
an absolute path or portable executable/symlink metadata.

For providers, resolve the clone URL through the authenticated provider API and
fetch the exact selected SHA with depth one, without checkout or submodule
recursion. Use the configured GitHub/GitLab origin, prohibit credential-bearing
URLs and redirects, and pass credentials only through the Git process environment.
Provider archives can apply export attributes or omit submodule contents; native
Git preserves the recorded tree. Reject submodules because the repository must
contain its complete code. Do not fetch LFS objects or external repositories.

Keep provider source/branch/SHA as provenance. Build an independent parentless
commit from the unchanged imported tree before saving, so snapshots do not depend
on remote history. Recheck project editing rights when publishing and retain the
existing immutable-reference and database-transaction protocol.

Default limits are 100 MiB actual contents and 10,000 files, configurable in the
API environment. Bound simultaneous staging jobs, provider transfer storage, and
provider duration. Discard staging on cancellation, save, explicit discard, or
30 minutes without access. Keep the previous editor contents until replacement
succeeds, ask before replacing unsaved work, and warn before leaving the page.
Draft recovery across visits and deployment from historical commits are deferred.
