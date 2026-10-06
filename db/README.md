# Database development

The database uses one current schema during development. Keep table definitions
in `schema.sql`, deployment functions and triggers in `deployment_rules.sql`, and
demo data in `populate.sql`. There are no historical upgrade migrations.

```text
db/
  schema.sql                 Tables, enums, constraints, and indexes
  deployment_rules.sql       Permission checks and deployment lifecycle operations
  populate.sql               Demo users, organizations, projects, and deployments
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

# Initialize an empty database schema. Includes deployment_rules.sql.
./db/scripts/setup_db.sh

# Populate demo data and prepare the matching snapshot files.
./db/scripts/populate_db.sh

# Test in a temporary schema and temporary snapshot directory.
./db/scripts/test_db.sh
```

Setup refuses to run when tables already exist and never clears existing data.
The existing development database is already initialized, so only run population
to add demo data. Population can be rerun without duplicating fixtures or resetting
passwords. During schema development, rebuild a disposable database explicitly
before running setup again.

Snapshots default to `var/snapshots/`, outside frontend assets and Git tracking.
Set an absolute `SNAPSHOT_STORAGE_DIR` in `.env` to override that location. Use the
population script rather than executing `populate.sql` directly: it writes the
snapshot archives first and supplies their real hashes to SQL.
