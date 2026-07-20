#!/usr/bin/env bash
set -euo pipefail

root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo 'ERROR: run this inside a Git repository' >&2
  exit 1
}
cd "$root"

private_patterns_file="${SHELLBELL_PRIVATE_PATTERNS_FILE:-}"
allowlist_file="${SHELLBELL_HISTORY_ALLOWLIST_FILE:-}"
failed=0

filter_allowlist() {
  local output="$1"
  local pattern

  if [[ -z "$allowlist_file" ]]; then
    printf '%s' "$output"
    return 0
  fi

  [[ -f "$allowlist_file" ]] || {
    echo "ERROR: history allowlist file does not exist: $allowlist_file" >&2
    exit 1
  }

  while IFS= read -r pattern; do
    [[ -n "$pattern" && "$pattern" != \#* ]] || continue
    output="$(printf '%s\n' "$output" | grep -Ev "$pattern" || true)"
  done < "$allowlist_file"

  printf '%s' "$output"
}

scan() {
  local label="$1"
  local pattern="$2"
  local output

  output="$(
    while read -r revision; do
      git grep -I -nE "$pattern" "$revision" -- \
        . \
        ':!Cargo.lock' \
        ':!scripts/audit-history.sh' \
        ':!scripts/check-public-content.sh' \
        ':!scripts/check-site.sh' \
        2>/dev/null || true
    done < <(git rev-list --all) | sort -u
  )"
  output="$(filter_allowlist "$output")"

  if [[ -n "$output" ]]; then
    printf '\n%s\n%s\n' "$label" "$output"
    failed=1
  fi
}

scan 'Absolute user-home paths found in history:' \
  '(/home/[A-Za-z0-9._-]+/|[A-Za-z]:\\Users\\[A-Za-z0-9._-]+\\)'
scan 'Private-key or token-shaped content found in history:' \
  'BEGIN (RSA|OPENSSH|EC) PRIVATE KEY|gh[pousr]_[A-Za-z0-9]{36,}'
scan 'Cloud-provider-specific deployment wording found in history:' \
  '(Amazon Lightsail|DigitalOcean|Linode|Vultr|Hetzner)'

if [[ -n "$private_patterns_file" ]]; then
  [[ -f "$private_patterns_file" ]] || {
    echo "ERROR: private patterns file does not exist: $private_patterns_file" >&2
    exit 1
  }
  while IFS= read -r pattern; do
    [[ -n "$pattern" && "$pattern" != \#* ]] || continue
    scan "Private pattern found in history: $pattern" "$pattern"
  done < "$private_patterns_file"
fi

if [[ "$failed" -ne 0 ]]; then
  printf '\nHistory audit found unresolved content requiring review.\n' >&2
  exit 1
fi

if [[ -n "$allowlist_file" ]]; then
  printf '%s\n' 'Git history privacy audit passed with explicit reviewed exceptions.'
else
  printf '%s\n' 'Git history privacy audit passed.'
fi
