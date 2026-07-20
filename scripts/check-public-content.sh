#!/usr/bin/env bash
set -euo pipefail

paths=(
  README.md
  CHANGELOG.md
  ROADMAP.md
  CONTRIBUTING.md
  SECURITY.md
  CODE_OF_CONDUCT.md
  install.sh
  scripts/relay-backup.sh
  scripts/relay-restore.sh
  scripts/diagnose.sh
  scripts/test-installer.sh
  site
  docs
  deploy
  .github
)

failed=0

check_matches() {
  local title="$1"
  local pattern="$2"
  local matches

  matches="$(git grep -niE "$pattern" -- "${paths[@]}" 2>/dev/null || true)"
  if [[ -n "$matches" ]]; then
    printf '%s\n%s\n' "$title" "$matches"
    failed=1
  fi
}

check_matches \
  'Absolute user home paths found in public content:' \
  '(/home/[A-Za-z0-9._-]+/|[A-Za-z]:\\Users\\[A-Za-z0-9._-]+\\)'

check_matches \
  'Cloud-provider-specific wording found in public content:' \
  '(Amazon Lightsail|DigitalOcean|Linode|Vultr|Hetzner)'

email_matches="$(
  git grep -niE '[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}' -- "${paths[@]}" 2>/dev/null \
    | grep -vEi '@example\.(com|org|net)' \
    || true
)"
if [[ -n "$email_matches" ]]; then
  printf '%s\n%s\n' 'Non-example email addresses found in public content:' "$email_matches"
  failed=1
fi

ip_matches="$(
  git grep -nE '([0-9]{1,3}\.){3}[0-9]{1,3}' -- "${paths[@]}" 2>/dev/null \
    | grep -vE '(127\.0\.0\.1|0\.0\.0\.0|192\.0\.2\.|198\.51\.100\.|203\.0\.113\.)' \
    || true
)"
if [[ -n "$ip_matches" ]]; then
  printf '%s\n%s\n' 'Non-example IP addresses found in public content:' "$ip_matches"
  failed=1
fi

if (( failed )); then
  printf '%s\n' 'Public-content privacy check failed.'
  exit 1
fi

printf '%s\n' 'Public-content privacy check passed.'
