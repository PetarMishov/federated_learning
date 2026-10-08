#!/usr/bin/env bash
set -euo pipefail
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source "$script_dir/lib/database.sh"
configure_database "$@"
test_tempdir="$(mktemp -d)"
test_schema=""
cleanup() {
  if [[ -n "$test_schema" ]]; then
    run_psql --quiet --command="SET client_min_messages=warning; DROP SCHEMA IF EXISTS $test_schema CASCADE;" >/dev/null
  fi
  rm -rf -- "$test_tempdir"
}
trap cleanup EXIT
source "$script_dir/lib/git_snapshots.sh"

test_schema="fl_test_${BASHPID}_${RANDOM}"
run_psql --quiet --command="CREATE SCHEMA $test_schema;"
PGOPTIONS="-c search_path=$test_schema" "$script_dir/setup_db.sh" "$@"
if setup_error="$(PGOPTIONS="-c search_path=$test_schema" "$script_dir/setup_db.sh" "$@" 2>&1)"; then
  echo "Setup unexpectedly accepted an already populated schema." >&2
  exit 1
fi
if [[ "$setup_error" != *"Setup requires an empty schema."* ]]; then
  echo "Unexpected setup failure: $setup_error" >&2
  exit 1
fi
for iteration in 1 2; do
  PGOPTIONS="-c search_path=$test_schema" run_psql --quiet --file="$script_dir/../populate.sql"
  PGOPTIONS="-c search_path=$test_schema" populate_git_snapshots "$test_tempdir/git"
done
run_psql --quiet --command="SET search_path TO $test_schema;" \
  --file="$script_dir/../tests/deployment_lifecycle.sql"
python3 "$script_dir/../tests/test_git_snapshots.py"
echo "Database and Git snapshot checks passed."
