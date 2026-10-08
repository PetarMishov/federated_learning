#!/usr/bin/env bash
# Uses the connection configured by database.sh. Git publication precedes SQL.
populate_git_snapshots() {
  local git_storage_dir="$1" manifest_file commits snapshot_json
  if [[ "$git_storage_dir" != /* ]]; then
    echo "GIT_STORAGE_DIR must be an absolute path." >&2
    return 1
  fi
  commits="$(python3 "$script_dir/lib/git_snapshots.py" hashes)"
  local commit_array
  mapfile -t commit_array <<< "$commits"
  manifest_file="$(mktemp)"
  if ! run_psql --quiet --set="v1=${commit_array[0]}" --set="v2=${commit_array[1]}" > "$manifest_file" <<'SQL'
COPY (
    SELECT p.id,
           COALESCE((SELECT s.id FROM snapshots s
                     WHERE s.project_id = p.id AND s.git_commit_sha = v.sha
                     ORDER BY s.id LIMIT 1),
                    nextval(pg_get_serial_sequence('snapshots', 'id'))),
           o.owner_user_id, v.version, v.sha
    FROM projects p JOIN organizations o ON o.id = p.org_id
    JOIN users u ON u.id = o.owner_user_id AND u.username = 'demo'
    CROSS JOIN (VALUES ('v1', :'v1'), ('v2', :'v2')) v(version, sha)
    WHERE (o.name, p.name) IN (
        ('Central Hospital', 'Patient risk prediction'),
        ('Central Hospital', 'Medical image classification'),
        ('Research Lab', 'Federated learning benchmark'),
        ('Research Lab', 'Privacy-preserving model evaluation'),
        ('Research Lab', 'Training algorithm comparison'),
        ('Medical Network', 'Cross-hospital outcome prediction'))
      AND (v.version = 'v1' OR p.name = 'Patient risk prediction')
    ORDER BY p.id, v.version
) TO STDOUT WITH CSV;
SQL
  then
    rm -f -- "$manifest_file"
    return 1
  fi
  if ! snapshot_json="$(python3 "$script_dir/lib/git_snapshots.py" publish "$git_storage_dir" "$manifest_file")"; then
    rm -f -- "$manifest_file"
    return 1
  fi
  rm -f -- "$manifest_file"
  run_psql --quiet --set="demo_snapshots_json=$snapshot_json" --file="$script_dir/../populate_deployments.sql"
}
