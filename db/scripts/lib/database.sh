#!/usr/bin/env bash
# Shared connection setup. Parse settings without executing .env as shell code.
configure_database() {
  postgres_env_file="$script_dir/../../.env"
  if [[ $# -eq 2 && "$1" == "--env-file" ]]; then
    postgres_env_file="$2"
  elif [[ $# -ne 0 ]]; then
    echo "Usage: $0 [--env-file PATH]" >&2
    return 1
  fi
  if [[ ! -r "$postgres_env_file" ]]; then
    echo "Environment file is not readable: $postgres_env_file" >&2
    return 1
  fi
  if ! command -v psql >/dev/null; then
    echo "Install the PostgreSQL client (psql) before running this script." >&2
    return 1
  fi
  postgres_user="$(read_setting POSTGRES_USER)"
  postgres_database="$(read_setting POSTGRES_DB)"
  postgres_port="$(read_setting POSTGRES_PORT)"
  postgres_password="$(read_setting POSTGRES_PASSWORD)"
  if [[ -z "$postgres_user" || -z "$postgres_database" || -z "$postgres_password" ]]; then
    echo "POSTGRES_USER, POSTGRES_DB, and POSTGRES_PASSWORD must be set in $postgres_env_file" >&2
    return 1
  fi
  if [[ ! "$postgres_port" =~ ^[0-9]+$ ]] || (( postgres_port < 1 || postgres_port > 65535 )); then
    echo "POSTGRES_PORT must be a number between 1 and 65535" >&2
    return 1
  fi
}

read_setting() {
  local value
  value="$(awk -v key="$1" 'index($0, key "=") == 1 { value = substr($0, length(key) + 2); sub(/\r$/, "", value); print value; exit }' "$postgres_env_file")"
  if [[ "$value" == \"*\" || "$value" == \'*\' ]]; then
    value="${value:1:${#value}-2}"
  fi
  printf '%s' "$value"
}

run_psql() {
  PGPASSWORD="$postgres_password" PGCONNECT_TIMEOUT=5 psql \
    --host=127.0.0.1 --port="$postgres_port" --username="$postgres_user" \
    --dbname="$postgres_database" --no-password --no-psqlrc --set=ON_ERROR_STOP=on "$@"
}
