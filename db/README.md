# Database development

The database uses one current schema during development. Keep table definitions
in `schema.sql`, deployment functions and triggers in `deployment_rules.sql`, and
demo data in `populate.sql` and `populate_deployments.sql`.
See the [database diagram](../docs/db/db_diagram.md) and its
[DBML source](../docs/db/db_diagram.txt).

```text
db/
  schema.sql                 Tables, enums, constraints, and indexes
  deployment_rules.sql       Permission checks and deployment lifecycle operations
  populate.sql               Demo users, organizations, and projects
  populate_deployments.sql    Snapshot metadata and deployment fixtures
  fixtures/snapshots/        Example project files captured into snapshots
  scripts/                   Database startup, setup, population, and testing
    lib/                     Shared connection and snapshot helpers
  tests/                     Current lifecycle and snapshot storage checks
```

Run commands from the repository root. Scripts read the root `.env`; an optional
`--env-file PATH` selects a different configuration.

```bash
# Start the development PostgreSQL container.
./db/scripts/run_postgres_server.sh

# Create the database if missing, then initialize its empty schema.
# Includes deployment_rules.sql.
./db/scripts/setup_db.sh

# Populate demo data and prepare retained Git snapshots.
./db/scripts/populate_db.sh

# Test in a temporary schema and temporary Git storage directory.
./db/scripts/test_db.sh

# Test recreation of a missing database and refusal to overwrite its tables.
# Creates and drops its own temporary database; requires CREATEDB permission.
python3 db/tests/test_setup_database.py

# Permanently remove the configured database (asks for confirmation).
./db/scripts/remove_db.sh
```

Setup creates the configured database if it is missing, using the `postgres`
maintenance database. The configured PostgreSQL user needs permission to create
databases. Setup refuses to run when tables already exist and never clears
existing data. Population can be rerun without duplicating fixtures or resetting
passwords. During schema development, rebuild a disposable database explicitly
before running setup again.

Demo population includes 25 deployments across six projects and three
organizations. Every project has at least four deployments and three different
states; all six deployment states appear across the fixtures. Pending runs also
vary in whether zero, one, or two participants have joined.

Removal drops the entire configured database and disconnects its active clients.
It leaves the PostgreSQL container, other databases, and local snapshot files in
place. Run setup and population again to recreate the database and demo data:

```bash
./db/scripts/setup_db.sh
./db/scripts/populate_db.sh
```

Snapshots are Git commits retained at `refs/snapshots/<snapshot-id>` in
`storage/git/projects/<project-id>.git`, outside frontend assets and Git tracking.
Set an absolute `GIT_STORAGE_DIR` in `.env` to override the Git root.
Population requires Git and Python 3. Use `populate_db.sh` rather than executing
SQL directly: it creates projects, reserves snapshot IDs, publishes their commits
and refs, then commits snapshot metadata and deployment fixtures. Reruns verify
existing refs without overwriting them. Failed metadata publication can leave
unused Git refs for later reconciliation.

See [Git storage](../docs/git-storage.md) for details.
