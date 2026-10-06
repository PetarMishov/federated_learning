#!/usr/bin/env bash
set -euo pipefail
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source "$script_dir/lib/database.sh"
configure_database "$@"

if ! command -v python3 >/dev/null; then
  echo "Python 3 is required to prepare the demo snapshot archives." >&2
  exit 1
fi
snapshot_storage_dir="$(read_setting SNAPSHOT_STORAGE_DIR)"
snapshot_storage_dir="${snapshot_storage_dir:-$script_dir/../../var/snapshots}"
if [[ "$snapshot_storage_dir" != /* ]]; then
  echo "SNAPSHOT_STORAGE_DIR must be an absolute path." >&2
  exit 1
fi
snapshot_hashes="$(python3 "$script_dir/lib/snapshots.py" "$snapshot_storage_dir")"
mapfile -t snapshot_hash_array <<< "$snapshot_hashes"
if [[ ${#snapshot_hash_array[@]} -ne 2 || ! ${snapshot_hash_array[0]} =~ ^[a-f0-9]{64}$ || ! ${snapshot_hash_array[1]} =~ ^[a-f0-9]{64}$ ]]; then
  echo "Could not prepare the demo snapshot hashes." >&2
  exit 1
fi

echo "Populating $postgres_database on 127.0.0.1:$postgres_port as $postgres_user…"
run_psql --set="snapshot_v1_sha256=${snapshot_hash_array[0]}" \
  --set="snapshot_v2_sha256=${snapshot_hash_array[1]}" --file="$script_dir/../populate.sql"
echo "Demo data populated. Snapshots are stored in $snapshot_storage_dir."
