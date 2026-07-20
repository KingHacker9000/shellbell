#!/usr/bin/env bash
set -euo pipefail

for file in site/index.html site/styles.css site/app.js; do
  [[ -s "$file" ]] || {
    echo "missing documentation-site file: $file" >&2
    exit 1
  }
done

grep -Fq 'id="relay-url"' site/index.html
grep -Fq 'data-copy' site/index.html
grep -Fq 'renderCommands' site/app.js

if grep -RniE '(/home/[A-Za-z0-9._-]+/|[A-Za-z]:\\Users\\)' site; then
  echo 'absolute user path found in documentation site' >&2
  exit 1
fi

relay_matches="$(
  grep -RniE 'https://shellbell\.[A-Za-z0-9.-]+\.[A-Za-z]{2,}' site 2>/dev/null |
    grep -vF 'https://shellbell.example.com' || true
)"
if [[ -n "$relay_matches" ]]; then
  printf '%s\n%s\n' 'non-example Shellbell relay URL found in documentation site:' "$relay_matches" >&2
  exit 1
fi

printf '%s\n' 'documentation site validation passed'
