#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
postgres_env_file="$script_dir/../../.env"

if [[ $# -eq 2 && "$1" == "--env-file" ]]; then
  postgres_env_file="$2"
elif [[ $# -ne 0 ]]; then
  echo "Usage: $0 [--env-file PATH]" >&2
  exit 1
fi

if [[ ! -r "$postgres_env_file" ]]; then
  echo "Environment file is not readable: $postgres_env_file" >&2
  exit 1
fi

postgres_port="$(awk -F= '$1 == "POSTGRES_PORT" { sub(/\r$/, "", $2); print $2 }' "$postgres_env_file")"
if [[ ! "$postgres_port" =~ ^[0-9]+$ ]] || (( postgres_port < 1 || postgres_port > 65535 )); then
  echo "POSTGRES_PORT must be a number between 1 and 65535" >&2
  exit 1
fi

if ! command -v docker >/dev/null; then
  echo "Docker must be installed to run PostgreSQL." >&2
  exit 1
fi

if docker container inspect federated-learning-db >/dev/null 2>&1; then
  docker start federated-learning-db >/dev/null
else
  docker run --name federated-learning-db \
    --env-file "$postgres_env_file" \
    -p "127.0.0.1:$postgres_port:5432" \
    -v federated-learning-pgdata:/var/lib/postgresql/data \
    -d postgres:17 >/dev/null
fi

for attempt in {1..30}; do
  if docker exec federated-learning-db pg_isready -h 127.0.0.1 -p 5432 >/dev/null 2>&1; then
    echo "PostgreSQL is ready. Connect pgAdmin to 127.0.0.1 using this published port:"
    docker port federated-learning-db 5432/tcp
    echo "Use POSTGRES_DB, POSTGRES_USER, and POSTGRES_PASSWORD from your environment file."
    echo "Existing containers keep their original port and credentials."
    exit 0
  fi
  sleep 1
done

echo "PostgreSQL did not become ready. Check: docker logs federated-learning-db" >&2
exit 1
