# Frontend

This project was generated using [Angular CLI](https://github.com/angular/angular-cli) version 22.2.1.

## Development server

To start a local development server, run:

```bash
ng serve
```

Once the server is running, open your browser and navigate to `http://localhost:4200/`. The application will automatically reload whenever you modify any of the source files.

## Code scaffolding

Angular CLI includes powerful code scaffolding tools. To generate a new component, run:

```bash
ng generate component component-name
```

For a complete list of available schematics (such as `components`, `directives`, or `pipes`), run:

```bash
ng generate --help
```

## Building

To build the project run:

```bash
ng build
```

This will compile your project and store the build artifacts in the `dist/` directory. By default, the production build optimizes your application for performance and speed.

## Running unit tests

To execute unit tests with the [Vitest](https://vitest.dev/) test runner, use the following command:

```bash
ng test
```

## Running end-to-end tests

For end-to-end (e2e) testing, run:

```bash
ng e2e
```

Angular CLI does not come with an end-to-end testing framework by default. You can choose one that suits your needs.

## Additional Resources

For more information on using the Angular CLI, including detailed command references, visit the [Angular CLI Overview and Command Reference](https://angular.dev/tools/cli) page.

## API connection

Run the Rust API on port 3000 and this frontend with `npm start`. The development
proxy forwards `/users/**` and the organization project/member endpoints to the API. Both the API and the PostgreSQL startup
script use the root `.env`; no `api/.env` copy is needed.

The initial page is `/login`, without a sidebar. Sign in with `demo` /
`demo-password` after running the population script. Successful login opens
`/home`; unauthenticated visits to that route redirect to login. Tokens are
stored in session storage. Expired sessions require signing in again.

Logout calls `POST /users/logout` with the bearer token, revokes that token in
PostgreSQL, clears the browser session, and returns to login. Network failures
keep the session available so logout can be retried. Each login gets a unique
JWT identifier. Tokens issued before this change require signing in again.

For database setup and population, see [db/README.md](../db/README.md).
The current schema includes the revoked-token table, which stores identifiers
and expiry timestamps rather than bearer tokens. Revocations survive API restarts.

For production hosting, route `/users/**` to the API through your web server;
the Angular development proxy is only used by `ng serve`. Notifications load from the API.

## Repository selection

The project import controls include a searchable repository dropdown for GitHub
and GitLab. Opening it loads the caller's saved connection's repositories, fetching
all pages automatically. Search matches namespace/owner and repository names
without case sensitivity. The list scrolls and supports arrow keys, Enter, and
Escape. Switching source clears the selection and cancels outstanding requests;
local folder imports have no repository selector.

The API serves `GET /connectors/{github|gitlab}/repositories?page=1`. Missing or
rejected provider credentials produce an explanatory message and retry control.
GitHub and GitLab connections can be created through the Connectors page. Press Load project to open the selected commit as an editable, unsaved draft.

## Connectors page

The sidebar links to `/connectors`, protected by the existing login guard.
Personal access token forms submit to
`POST /connectors/gitlab/authorize` and `POST /connectors/github/authorize`. Tokens remain masked without a reveal control, are cleared
after a successful save, and never persisted in browser storage. The page displays
the returned account name and explains failures so users can retry. Saved status
reflects saves in the current visit; there is no connection-status endpoint to
restore it after navigating away. Both forms have independent saving and error
states. GitHub users should select repositories on a fine-grained token and
grant Contents read-only permission; classic tokens can use `repo` for private
repositories. Configure `CONNECTOR_TOKEN_KEY` in the API to enable both providers.

## Branch and commit selection

After selecting a repository, open the branch dropdown to load and search all
branch names, with the same keyboard navigation, pagination, retry, and dismissal
behavior as the repository picker. Selecting a branch fills the commit field with
its newest (head) commit SHA. The SHA remains editable for choosing a historical
commit. Changing the repository/provider clears branch and commit selections;
changing branches selects the new branch's head.

GitLab tokens require `read_api` and `read_repository`; replace older saved tokens
that only have `read_user` and `read_repository`. GitHub fine-grained tokens need
Contents read permission. Load project imports the selected commit. Deployment behavior is unchanged.

## Local folder selection

Local Folder has a read-only folder display and a compact Browse button opening
the browser's directory chooser. Browsers expose the folder name and selected
files, not the absolute path. Cancelling the chooser preserves the selection.
Load project honors nested `.gitignore` rules and excludes `.git` before streaming
files. Folder selection alone does not upload anything.

## Loading projects

Load project supports local folders and exact GitHub/GitLab commits. A compact
progress bar and cancel control appear on the right of Saved snapshots. Transfers
show measured percentages when available; preparation uses an indeterminate bar.
The previous editor remains available until loading succeeds. Replacing unsaved
work requires confirmation, and a failed/cancelled load preserves it.

Loaded projects appear as Unsaved draft and use the existing file editor. Save
snapshot publishes the draft even without edits. Files and directories load on
demand; binary and large files remain included even when the editor cannot preview
them. Provider symlinks stay links; local browser uploads contain regular files
with no portable executable/symlink metadata. Provider submodules are rejected.

The API defaults to 100 MiB and 10,000 files; configure
`PROJECT_IMPORT_MAX_BYTES` and `PROJECT_IMPORT_MAX_FILES` in the root `.env` and
restart the API. Size counts actual file contents rather than transfer overhead.
Provider transfer speed and local upload speed still depend on the network.

Drafts last for the current visit; leaving/refreshing warns about unsaved work.
The API removes abandoned staging after 30 minutes without access. Saving,
discarding, or cancellation releases staging. See the [import contract](../docs/project-import.md).
