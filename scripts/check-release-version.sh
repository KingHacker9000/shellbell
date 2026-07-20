#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <release-tag>" >&2
  exit 2
fi

tag="$1"
if [[ ! "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
  echo "invalid release tag: $tag" >&2
  exit 1
fi

release_version="${tag#v}"
base_version="${release_version%%-*}"

cargo_version="$({
  awk '
    /^\[workspace\.package\]$/ { in_package = 1; next }
    /^\[/ { in_package = 0 }
    in_package && /^version = / {
      gsub(/^[^"]*"|".*$/, "")
      print
      exit
    }
  ' Cargo.toml
} )"

pwa_version="$(node -p "require('./pwa/package.json').version")"

if [[ -z "$cargo_version" ]]; then
  echo "could not read workspace package version" >&2
  exit 1
fi

if [[ "$cargo_version" != "$base_version" ]]; then
  echo "tag base version $base_version does not match Cargo version $cargo_version" >&2
  exit 1
fi

if [[ "$pwa_version" != "$base_version" ]]; then
  echo "tag base version $base_version does not match PWA version $pwa_version" >&2
  exit 1
fi

if ! grep -Fq "## [$release_version]" CHANGELOG.md && ! grep -Fq "## [$base_version]" CHANGELOG.md; then
  echo "CHANGELOG.md has no section for $release_version or $base_version" >&2
  exit 1
fi

printf 'release version verified: %s\n' "$release_version"
