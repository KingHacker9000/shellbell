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

binary_version_output="$("$binary" --version)" || {
  echo "binary could not report its version: $binary" >&2
  exit 1
}
case "$binary_version_output" in
  shellbell\ *) binary_version="${binary_version_output#shellbell }" ;;
  *)
    echo "unexpected binary version output: $binary_version_output" >&2
    exit 1
    ;;
esac
[[ "$binary_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || {
  echo "unexpected binary version: $binary_version" >&2
  exit 1
}

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

version="${binary_version}-test.1"
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

before_hash="$(sha256sum "$prefix/bin/shellbell" | awk '{print $1}')"

bad_version="${binary_version}-test.2"
bad_release="$tmp/bad-release"
bad_binary="$tmp/bad-shellbell"
mkdir -p "$bad_release"
cat > "$bad_binary" <<'BAD'
#!/bin/sh
echo 'simulated incompatible executable' >&2
exit 1
BAD
chmod 0755 "$bad_binary"

bash "$root/scripts/package-release.sh" \
  "$bad_binary" \
  linux-x86_64 \
  "$bad_version" \
  "$bad_release"
cat "$bad_release"/*.sha256 > "$bad_release/SHA256SUMS"

if SHELLBELL_RELEASE_DIR="$bad_release" \
  SHELLBELL_VERSION="$bad_version" \
  SHELLBELL_PREFIX="$prefix" \
  sh "$root/install.sh" >/dev/null 2>&1
then
  echo 'installer accepted a binary that cannot execute' >&2
  exit 1
fi

after_hash="$(sha256sum "$prefix/bin/shellbell" | awk '{print $1}')"
[[ "$before_hash" == "$after_hash" ]] || {
  echo 'installer replaced the existing binary before compatibility validation' >&2
  exit 1
}
"$prefix/bin/shellbell" --version

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
