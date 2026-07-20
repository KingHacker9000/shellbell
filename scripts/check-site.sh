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

if grep -RniE 'shellbell\.ashishajin\.com|/home/[A-Za-z0-9._-]+/|[A-Za-z]:\\Users\\' site; then
  echo 'private deployment content found in documentation site' >&2
  exit 1
fi

printf '%s\n' 'documentation site validation passed'
