#!/usr/bin/env bash
set -euo pipefail

stack="${SHELLBELL_STACK_DIR:-/opt/stacks/shellbell}"
service="${SHELLBELL_SERVICE:-shellbell}"
backup=""
confirmed=0

usage() {
  cat <<'USAGE'
Restore a Shellbell relay backup with an automatic pre-restore snapshot.

Usage:
  relay-restore.sh --backup DIR [--stack DIR] [--service NAME] --yes

The command verifies checksums, creates a fresh safety backup, restores the
Compose file, .env, and /data contents, then waits for container health.
USAGE
}

fail() {
  printf 'ERROR: %s\n' "$*" >&2
  exit 1
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --backup)
      [[ $# -ge 2 ]] || fail '--backup requires a value'
      backup="$2"
      shift 2
      ;;
    --stack)
      [[ $# -ge 2 ]] || fail '--stack requires a value'
      stack="$2"
      shift 2
      ;;
    --service)
      [[ $# -ge 2 ]] || fail '--service requires a value'
      service="$2"
      shift 2
      ;;
    --yes)
      confirmed=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *) fail "unknown option: $1" ;;
  esac
done

[[ -n "$backup" ]] || fail '--backup is required'
[[ "$confirmed" -eq 1 ]] || fail 'restore requires --yes'
for file in compose.yaml .env data.tar.gz metadata.txt SHA256SUMS; do
  [[ -f "$backup/$file" ]] || fail "backup file is missing: $file"
done

(
  cd "$backup"
  sha256sum -c SHA256SUMS
) || fail 'backup checksum verification failed'

compose="$stack/compose.yaml"
[[ -f "$compose" ]] || fail "$compose was not found"
[[ -f "$stack/.env" ]] || fail "$stack/.env was not found"

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
stamp="$(date -u +%Y%m%dT%H%M%SZ)"
safety="$stack/backups/pre-restore-$stamp"

"$script_dir/relay-backup.sh" \
  --stack "$stack" \
  --service "$service" \
  --output "$safety"

cd "$stack"
container_id="$(docker compose ps -aq "$service" | head -n 1)"
[[ -n "$container_id" ]] || fail "Compose service was not found: $service"

data_source="$(
  docker inspect "$container_id" \
    --format '{{range .Mounts}}{{if eq .Destination "/data"}}{{.Source}}{{end}}{{end}}'
)"
[[ -n "$data_source" && -d "$data_source" ]] || fail 'the /data mount source was not found'
[[ "$data_source" != '/' ]] || fail 'refusing to restore into the filesystem root'

restore_snapshot() {
  local snapshot="$1"
  docker compose stop "$service" >/dev/null 2>&1 || true
  sudo find "$data_source" -mindepth 1 -maxdepth 1 -exec rm -rf -- {} +
  sudo tar -C "$data_source" -xzf "$snapshot/data.tar.gz"
  sudo cp -a "$snapshot/compose.yaml" "$compose"
  sudo cp -a "$snapshot/.env" "$stack/.env"
  sudo chmod 600 "$stack/.env"
  docker compose config --quiet
  docker compose up -d --no-build --force-recreate "$service"
}

printf 'Restoring backup: %s\n' "$backup"
restore_snapshot "$backup"

container_id="$(docker compose ps -q "$service")"
health=''
for _ in {1..40}; do
  health="$(
    docker inspect "$container_id" \
      --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}{{.State.Status}}{{end}}' \
      2>/dev/null || true
  )"
  printf 'Health: %s\n' "${health:-missing}"
  [[ "$health" == 'healthy' || "$health" == 'running' ]] && break
  sleep 3
done

if [[ "$health" != 'healthy' && "$health" != 'running' ]]; then
  docker logs --tail 150 "$container_id" 2>&1 || true
  printf '%s\n' 'Restore failed; applying the automatic safety snapshot.' >&2
  restore_snapshot "$safety" || fail 'restore and automatic rollback both failed'
  fail 'restore failed; previous state was restored'
fi

curl -fsS http://127.0.0.1:8080/health >/dev/null \
  || fail 'restored relay did not pass the local health check'

printf 'Restore complete. Safety backup retained at: %s\n' "$safety"
