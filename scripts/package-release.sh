#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 3 || $# -gt 4 ]]; then
  echo "usage: $0 <binary> <platform> <version> [output-directory]" >&2
  exit 2
fi

binary="$1"
platform="$2"
version="$3"
output_dir="${4:-dist}"

if [[ ! -x "$binary" ]]; then
  echo "release binary is missing or not executable: $binary" >&2
  exit 1
fi

case "$platform" in
  linux-x86_64|linux-aarch64) ;;
  *)
    echo "unsupported release platform: $platform" >&2
    exit 1
    ;;
esac

if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$ ]]; then
  echo "invalid release version: $version" >&2
  exit 1
fi

package="shellbell-${version}-${platform}"
staging="$(mktemp -d)"
trap 'rm -rf "$staging"' EXIT

mkdir -p "$staging/$package" "$output_dir"
install -m 0755 "$binary" "$staging/$package/shellbell"
install -m 0644 README.md LICENSE "$staging/$package/"

archive="$output_dir/$package.tar.gz"
tar \
  --sort=name \
  --mtime='UTC 1970-01-01' \
  --owner=0 \
  --group=0 \
  --numeric-owner \
  -C "$staging" \
  -cf - \
  "$package" | gzip -n > "$archive"

(
  cd "$output_dir"
  sha256sum "$(basename "$archive")" > "$(basename "$archive").sha256"
)

printf '%s\n' "$archive"
