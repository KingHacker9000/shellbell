#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <shellbell-binary>" >&2
  exit 2
fi

binary="$1"
[[ -x "$binary" ]] || {
  echo "binary is missing or not executable: $binary" >&2
  exit 1
}

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

version='0.1.0-test.1'
release_dir="$tmp/release"
prefix="$tmp/prefix"
mkdir -p "$release_dir"

bash "$root/scripts/package-release.sh" \
  "$binary" \
  linux-x86_64 \
  "$version" \
  "$release_dir"

cat "$release_dir"/*.sha256 > "$release_dir/SHA256SUMS"

SHELLBELL_RELEASE_DIR="$release_dir" \
SHELLBELL_VERSION="$version" \
SHELLBELL_PREFIX="$prefix" \
  sh "$root/install.sh"

"$prefix/bin/shellbell" --version

SHELLBELL_RELEASE_DIR="$release_dir" \
SHELLBELL_VERSION="$version" \
SHELLBELL_PREFIX="$prefix" \
  sh "$root/install.sh"

corrupt="$tmp/corrupt"
cp -a "$release_dir" "$corrupt"
printf 'corruption\n' >> "$corrupt/shellbell-${version}-linux-x86_64.tar.gz"

if SHELLBELL_RELEASE_DIR="$corrupt" \
  SHELLBELL_VERSION="$version" \
  SHELLBELL_PREFIX="$tmp/should-not-install" \
  sh "$root/install.sh" >/dev/null 2>&1
then
  echo 'installer accepted a corrupt archive' >&2
  exit 1
fi

printf '%s\n' 'installer acceptance test passed'
