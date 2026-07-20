#!/usr/bin/env bash
set -euo pipefail

stack="${SHELLBELL_STACK_DIR:-/opt/stacks/shellbell}"
service="${SHELLBELL_SERVICE:-shellbell}"
output=""

usage() {
  cat <<'USAGE'
Create a consistent Shellbell relay backup.

Usage:
  relay-backup.sh [--stack DIR] [--service NAME] [--output DIR]

The backup contains compose.yaml, .env, data.tar.gz, metadata.txt, and
SHA256SUMS. Secrets are copied but never printed.
USAGE
}

fail() {
  printf 'ERROR: %s\n' "$*" >&2
  exit 1
}

while [[ $# -gt 0 ]]; do
  case "$1" in
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
    --output)
      [[ $# -ge 2 ]] || fail '--output requires a value'
      output="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *) fail "unknown option: $1" ;;
  esac
done

command -v docker >/dev/null 2>&1 || fail 'docker is required'
command -v sha256sum >/dev/null 2>&1 || fail 'sha256sum is required'

compose="$stack/compose.yaml"
[[ -f "$compose" ]] || fail "$compose was not found"
[[ -f "$stack/.env" ]] || fail "$stack/.env was not found"

cd "$stack"
docker compose config --quiet || fail 'Compose configuration is invalid'

container_id="$(docker compose ps -aq "$service" | head -n 1)"
[[ -n "$container_id" ]] || fail "Compose service was not found: $service"

data_source="$(
  docker inspect "$container_id" \
    --format '{{range .Mounts}}{{if eq .Destination "/data"}}{{.Source}}{{end}}{{end}}'
)"
[[ -n "$data_source" && -d "$data_source" ]] || fail 'the /data mount source was not found'
[[ "$data_source" != '/' ]] || fail 'refusing to back up the filesystem root'

running="$(docker inspect "$container_id" --format '{{.State.Running}}')"
image_ref="$(docker inspect "$container_id" --format '{{.Config.Image}}')"
image_id="$(docker inspect "$container_id" --format '{{.Image}}')"
stamp="$(date -u +%Y%m%dT%H%M%SZ)"
output="${output:-$stack/backups/$stamp}"

sudo install -d -m 0700 -o "$(id -u)" -g "$(id -g)" "$output"
sudo cp -a "$compose" "$output/compose.yaml"
sudo cp -a "$stack/.env" "$output/.env"
sudo chmod 600 "$output/.env"

restart_needed=0
restart_service() {
  if [[ "$restart_needed" -eq 1 ]]; then
    docker compose start "$service" >/dev/null
    restart_needed=0
  fi
}
trap restart_service EXIT

if [[ "$running" == 'true' ]]; then
  docker compose stop "$service"
  restart_needed=1
fi

sudo tar -C "$data_source" -czf "$output/data.tar.gz" .
sudo test -s "$output/data.tar.gz" || fail 'data backup archive is empty'

sudo tee "$output/metadata.txt" >/dev/null <<META
created_at=$stamp
service=$service
image_ref=$image_ref
image_id=$image_id
data_destination=/data
META

(
  cd "$output"
  sudo sha256sum compose.yaml .env data.tar.gz metadata.txt |
    sudo tee SHA256SUMS >/dev/null
)

restart_service
trap - EXIT

printf 'Backup created: %s\n' "$output"
