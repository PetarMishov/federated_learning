#!/usr/bin/env bash
set -euo pipefail
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source "$script_dir/lib/database.sh"
configure_database "$@"

has_tables="$(run_psql --tuples-only --no-align --command="SELECT EXISTS (
  SELECT 1 FROM pg_tables WHERE schemaname = current_schema()
);")"
if [[ "$has_tables" == t ]]; then
  echo "Database schema already contains tables. Setup requires an empty schema." >&2
  exit 1
fi

echo "Setting up $postgres_database on 127.0.0.1:$postgres_port…"
run_psql --quiet --file="$script_dir/../schema.sql"
