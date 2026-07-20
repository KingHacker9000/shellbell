#!/usr/bin/env bash
set -uo pipefail

send_test=0
target='all'
wait_seconds=5

usage() {
  cat <<'USAGE'
Run safe Shellbell source diagnostics without printing credentials or Push data.

Usage:
  diagnose.sh [--send] [--to all|pc|phone] [--wait SECONDS]

--send queues a diagnostic notification and rechecks the durable queue after
an optional wait. Device display still requires human confirmation.
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --send) send_test=1; shift ;;
    --to)
      [[ $# -ge 2 ]] || { echo 'ERROR: --to requires a value' >&2; exit 2; }
      target="$2"
      shift 2
      ;;
    --wait)
      [[ $# -ge 2 && "$2" =~ ^[0-9]+$ ]] || {
        echo 'ERROR: --wait requires whole seconds' >&2
        exit 2
      }
      wait_seconds="$2"
      shift 2
      ;;
    -h|--help) usage; exit 0 ;;
    *) echo "ERROR: unknown option: $1" >&2; exit 2 ;;
  esac
done

case "$target" in all|pc|phone) ;; *) echo "ERROR: invalid target: $target" >&2; exit 2 ;; esac

failures=0
run_check() {
  local title="$1"
  shift
  printf '\n--- %s ---\n' "$title"
  if "$@"; then
    return 0
  fi
  failures=$((failures + 1))
  return 0
}

command -v shellbell >/dev/null 2>&1 || {
  echo 'ERROR: shellbell is not on PATH' >&2
  exit 1
}

printf '%s\n' 'Shellbell safe diagnostic report'
printf 'OS:   %s\n' "$(uname -s)"
printf 'Arch: %s\n' "$(uname -m)"
printf 'Time: %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"

run_check 'Version' shellbell --version
run_check 'Effective configuration' shellbell config
run_check 'Health checks' shellbell doctor

if [[ -n "${SHELLBELL_SESSION_ID:-}" ]]; then
  run_check 'Current shell session' shellbell status
else
  printf '\n--- Current shell session ---\n'
  printf '%s\n' 'Not running inside an integrated interactive shell.'
fi

if [[ "$send_test" -eq 1 ]]; then
  run_check 'Queue diagnostic notification' \
    shellbell ring "Shellbell diagnostic test $(date -u +%H:%M:%S)" --to "$target"
  sleep "$wait_seconds"
  run_check 'Queue after delivery attempt' shellbell doctor
  printf '\nConfirm that the notification appeared on the intended receiver(s).\n'
fi

printf '\nSummary: %d failed diagnostic command(s).\n' "$failures"
[[ "$failures" -eq 0 ]]
