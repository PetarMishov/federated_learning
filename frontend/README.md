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
# API connection

Run the Rust API on port 3000 and start this frontend with `npm start`.
The development proxy forwards `/users/**` requests to `http://127.0.0.1:3000`.
Restart `npm start` after changing the proxy configuration.

Sign in inside the My organizations panel. With the demo population script,
use username `demo` and password `demo-password`. The frontend stores the token
in session storage and sends it as a bearer token when loading organizations.
Expired sessions require signing in again. Notifications remain a placeholder.

For production hosting, route `/users/**` to the API through your web server;
the Angular development proxy is only used by `ng serve`.
