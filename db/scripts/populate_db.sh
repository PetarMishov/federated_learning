#!/usr/bin/env bash
set -euo pipefail
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source "$script_dir/lib/database.sh"
configure_database "$@"

source "$script_dir/lib/git_snapshots.sh"
git_storage_dir="$(read_setting GIT_STORAGE_DIR)"
git_storage_dir="${git_storage_dir:-$script_dir/../../storage/git}"

echo "Populating $postgres_database on 127.0.0.1:$postgres_port as $postgres_user…"
run_psql --file="$script_dir/../populate.sql"
populate_git_snapshots "$git_storage_dir"
echo "Demo data populated. Git snapshots are stored in $git_storage_dir/projects."
