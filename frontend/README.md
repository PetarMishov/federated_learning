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
GitHub and GitLab connections can be created through the Connectors page. Repository import itself is
still separate from selecting a repository.

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
