#!/bin/sh
set -eu

REPOSITORY="${SHELLBELL_REPOSITORY:-KingHacker9000/shellbell}"
PREFIX="${SHELLBELL_PREFIX:-$HOME/.local}"
VERSION="${SHELLBELL_VERSION:-latest}"
RELEASE_DIR="${SHELLBELL_RELEASE_DIR:-}"
RELEASE_BASE_URL="${SHELLBELL_RELEASE_BASE_URL:-}"
SHELLS=""
ALL_SHELLS=0
DRY_RUN=0

usage() {
  cat <<'USAGE'
Install or upgrade the latest stable Shellbell CLI release.

Usage:
  install.sh [--version VERSION] [--prefix DIR] [--shell SHELL]...
             [--all-shells] [--dry-run]

Options:
  --version VERSION  Install a specific version such as 0.1.0 or v0.1.0.
  --prefix DIR       Install under DIR/bin (default: $HOME/.local).
  --shell SHELL      Install integration for bash, zsh, or fish. Repeatable.
  --all-shells       Install all supported shell integrations.
  --dry-run          Resolve and verify the plan without changing the system.
  -h, --help         Show this help.

Environment overrides:
  SHELLBELL_VERSION
  SHELLBELL_PREFIX
  SHELLBELL_REPOSITORY
USAGE
}

fail() {
  printf 'ERROR: %s\n' "$*" >&2
  exit 1
}

need() {
  command -v "$1" >/dev/null 2>&1 || fail "required command is missing: $1"
}

append_shell() {
  case "$1" in
    bash|zsh|fish) ;;
    *) fail "unsupported shell: $1" ;;
  esac
  case " $SHELLS " in
    *" $1 "*) ;;
    *) SHELLS="${SHELLS}${SHELLS:+ }$1" ;;
  esac
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --version)
      [ "$#" -ge 2 ] || fail "--version requires a value"
      VERSION="$2"
      shift 2
      ;;
    --prefix)
      [ "$#" -ge 2 ] || fail "--prefix requires a value"
      PREFIX="$2"
      shift 2
      ;;
    --shell)
      [ "$#" -ge 2 ] || fail "--shell requires a value"
      append_shell "$2"
      shift 2
      ;;
    --all-shells)
      ALL_SHELLS=1
      shift
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      fail "unknown option: $1"
      ;;
  esac
done

[ "$(uname -s)" = "Linux" ] || fail "Shellbell release archives currently support Linux only"

case "$(uname -m)" in
  x86_64|amd64) PLATFORM='linux-x86_64' ;;
  aarch64|arm64) PLATFORM='linux-aarch64' ;;
  *) fail "unsupported Linux architecture: $(uname -m)" ;;
esac

need tar
need sha256sum
need install
need mktemp

if [ "$VERSION" = "latest" ]; then
  need curl
  latest_url="$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$REPOSITORY/releases/latest")" \
    || fail "could not resolve the latest Shellbell release"
  VERSION="${latest_url##*/}"
fi

VERSION="${VERSION#v}"
case "$VERSION" in
  ''|*[!0-9A-Za-z.-]*) fail "invalid version: $VERSION" ;;
esac

PACKAGE="shellbell-${VERSION}-${PLATFORM}"
ARCHIVE="$PACKAGE.tar.gz"
TARGET_DIR="$PREFIX/bin"
TARGET="$TARGET_DIR/shellbell"

printf 'Shellbell version: %s\n' "$VERSION"
printf 'Platform:          %s\n' "$PLATFORM"
printf 'Install target:    %s\n' "$TARGET"

if [ "$DRY_RUN" -eq 1 ]; then
  printf '%s\n' 'Dry run complete; no files were changed.'
  exit 0
fi

TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TEMP_DIR"' EXIT HUP INT TERM

if [ -n "$RELEASE_DIR" ]; then
  cp "$RELEASE_DIR/$ARCHIVE" "$TEMP_DIR/$ARCHIVE" \
    || fail "release archive was not found in $RELEASE_DIR"
  cp "$RELEASE_DIR/SHA256SUMS" "$TEMP_DIR/SHA256SUMS" \
    || fail "SHA256SUMS was not found in $RELEASE_DIR"
else
  need curl
  if [ -n "$RELEASE_BASE_URL" ]; then
    BASE_URL="${RELEASE_BASE_URL%/}"
  else
    BASE_URL="https://github.com/$REPOSITORY/releases/download/v$VERSION"
  fi
  curl -fL --retry 3 --retry-delay 1 \
    -o "$TEMP_DIR/$ARCHIVE" "$BASE_URL/$ARCHIVE" \
    || fail "could not download $ARCHIVE"
  curl -fL --retry 3 --retry-delay 1 \
    -o "$TEMP_DIR/SHA256SUMS" "$BASE_URL/SHA256SUMS" \
    || fail "could not download SHA256SUMS"
fi

EXPECTED_LINE="$TEMP_DIR/expected.sha256"
awk -v file="$ARCHIVE" '$2 == file { print; found = 1 } END { if (!found) exit 1 }' \
  "$TEMP_DIR/SHA256SUMS" > "$EXPECTED_LINE" \
  || fail "$ARCHIVE is missing from SHA256SUMS"

(
  cd "$TEMP_DIR"
  sha256sum -c "$(basename "$EXPECTED_LINE")"
) || fail "release checksum verification failed"

tar -xzf "$TEMP_DIR/$ARCHIVE" -C "$TEMP_DIR" \
  || fail "could not extract release archive"

BINARY="$TEMP_DIR/$PACKAGE/shellbell"
[ -x "$BINARY" ] || fail "release archive does not contain an executable shellbell binary"

mkdir -p "$TARGET_DIR"
TEMP_TARGET="$TARGET.tmp.$$"
install -m 0755 "$BINARY" "$TEMP_TARGET"
mv -f "$TEMP_TARGET" "$TARGET"

"$TARGET" --version

if [ "$ALL_SHELLS" -eq 1 ]; then
  "$TARGET" install --all-shells
else
  for selected_shell in $SHELLS; do
    "$TARGET" install --shell "$selected_shell"
  done
fi

case ":${PATH:-}:" in
  *":$TARGET_DIR:"*) ;;
  *)
    printf 'WARNING: %s is not currently on PATH.\n' "$TARGET_DIR" >&2
    printf 'Add it to your shell PATH before running shellbell.\n' >&2
    ;;
esac

printf '%s\n' 'Shellbell installation complete.'
printf '%s\n' 'Next: shellbell pair https://shellbell.example.com'
