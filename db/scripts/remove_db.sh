#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source "$script_dir/lib/database.sh"
configure_database "$@"

case "$postgres_database" in
  postgres|template0|template1)
    echo "Cannot remove PostgreSQL maintenance or template databases." >&2
    exit 1
    ;;
esac

echo "This will permanently delete database '$postgres_database' on 127.0.0.1:$postgres_port."
echo "Active connections to this database will be disconnected."
if ! read -r -p "Are you sure? [y/N] " confirmation; then
  echo "Cancelled."
  exit 0
fi
case "$confirmation" in
  y|Y|yes|YES|Yes) ;;
  *)
    echo "Cancelled."
    exit 0
    ;;
esac

# Connect to the maintenance database, not the database being removed.
# psql quotes database_name as an SQL identifier, including unusual characters.
run_psql --dbname=postgres --set=database_name="$postgres_database" <<'SQL'
DROP DATABASE :"database_name" WITH (FORCE);
SQL

echo "Removed database '$postgres_database'."
