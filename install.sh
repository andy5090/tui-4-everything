#!/usr/bin/env bash
# Install the portable T4E Linux release without requiring root privileges.
set -euo pipefail

REPOSITORY="${T4E_INSTALL_REPOSITORY:-andy5090/tui-4-everything}"
API_BASE="${T4E_INSTALL_API_BASE:-https://api.github.com}"
DOWNLOAD_BASE="${T4E_INSTALL_DOWNLOAD_BASE:-https://github.com}"
PREFIX="${HOME}/.local/bin"
REQUESTED_VERSION=""
UNINSTALL=false

usage() {
  cat <<'EOF'
Usage: install.sh [options]

Install the T4E portable Linux release into ~/.local/bin (no sudo required).

Options:
  --version VERSION  Install a specific release, for example 0.2.0.
  --prefix DIR       Install the t4e executable into DIR.
  --uninstall        Remove t4e from the selected prefix.
  -h, --help         Show this help message.

The installer supports x86_64, i686, and aarch64 Linux releases.
EOF
}

die() {
  printf 't4e installer: %s\n' "$*" >&2
  exit 1
}

cleanup() {
  [[ -n "${TEMP_DIR:-}" && -d "$TEMP_DIR" ]] && rm -rf -- "$TEMP_DIR"
}

download() {
  local url="$1" destination="$2"
  if command -v curl >/dev/null 2>&1; then
    curl --fail --location --retry 3 --connect-timeout 15 --output "$destination" "$url"
  elif command -v wget >/dev/null 2>&1; then
    wget --https-only --tries=3 --timeout=15 --output-document="$destination" "$url"
  else
    die "curl or wget is required to download T4E. Install one and try again."
  fi
}

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    die "sha256sum or shasum is required to verify the T4E download."
  fi
}

latest_version() {
  local metadata tag
  metadata="$TEMP_DIR/release.json"
  download "$API_BASE/repos/$REPOSITORY/releases/latest" "$metadata"
  tag="$(awk -F '"' '/"tag_name"/ { print $4; exit }' "$metadata")"
  [[ -n "$tag" ]] || die "could not determine the latest release version from GitHub; use --version VERSION."
  printf '%s\n' "${tag#v}"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      [[ $# -ge 2 ]] || die "--version requires a value"
      REQUESTED_VERSION="$2"
      shift 2
      ;;
    --prefix)
      [[ $# -ge 2 ]] || die "--prefix requires a directory"
      PREFIX="$2"
      shift 2
      ;;
    --uninstall) UNINSTALL=true; shift ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown option: $1 (run --help for usage)" ;;
  esac
done

[[ -n "$PREFIX" && "$PREFIX" != "/" ]] || die "--prefix must be a non-empty directory other than '/'."
[[ "$(uname -s)" == "Linux" ]] || die "this installer supports Linux only."
TARGET="$PREFIX/t4e"
if "$UNINSTALL"; then
  if [[ -e "$TARGET" || -L "$TARGET" ]]; then
    rm -f -- "$TARGET"
    printf 'Removed %s\n' "$TARGET"
  else
    printf 'T4E is not installed at %s\n' "$TARGET"
  fi
  exit 0
fi

case "$(uname -m)" in
  x86_64|amd64) ARCH="x86_64" ;;
  i386|i486|i586|i686) ARCH="i686" ;;
  aarch64|arm64) ARCH="aarch64" ;;
  *) die "unsupported CPU architecture: $(uname -m). Supported architectures: x86_64, i686, aarch64." ;;
esac

command -v tar >/dev/null 2>&1 || die "tar is required to unpack T4E. Install tar and try again."
TEMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/t4e-install.XXXXXX")" || die "could not create a temporary directory"
trap cleanup EXIT HUP INT TERM

if [[ -z "$REQUESTED_VERSION" ]]; then
  VERSION="$(latest_version)"
else
  VERSION="${REQUESTED_VERSION#v}"
fi
[[ "$VERSION" =~ ^[0-9][0-9A-Za-z._-]*$ ]] || die "invalid version '$REQUESTED_VERSION'"

PACKAGE="t4e-${VERSION}-linux-${ARCH}-musl"
ARCHIVE="$PACKAGE.tar.gz"
RELEASE_URL="$DOWNLOAD_BASE/$REPOSITORY/releases/download/v$VERSION"
ARCHIVE_PATH="$TEMP_DIR/$ARCHIVE"
CHECKSUM_PATH="$TEMP_DIR/$ARCHIVE.sha256"

printf 'Downloading T4E %s for Linux %s...\n' "$VERSION" "$ARCH"
download "$RELEASE_URL/$ARCHIVE" "$ARCHIVE_PATH"
download "$RELEASE_URL/$ARCHIVE.sha256" "$CHECKSUM_PATH"

EXPECTED_SHA="$(awk 'NR == 1 { print $1; exit }' "$CHECKSUM_PATH")"
CHECKSUM_FILENAME="$(awk 'NR == 1 { print $2; exit }' "$CHECKSUM_PATH")"
[[ "$EXPECTED_SHA" =~ ^[[:xdigit:]]{64}$ ]] || die "release checksum is malformed; refusing to install."
[[ "$CHECKSUM_FILENAME" == "$ARCHIVE" || "$CHECKSUM_FILENAME" == "*$ARCHIVE" ]] || die "release checksum does not match '$ARCHIVE'; refusing to install."
ACTUAL_SHA="$(sha256_file "$ARCHIVE_PATH")"
ACTUAL_SHA_LOWER="$(printf '%s' "$ACTUAL_SHA" | tr '[:upper:]' '[:lower:]')"
EXPECTED_SHA_LOWER="$(printf '%s' "$EXPECTED_SHA" | tr '[:upper:]' '[:lower:]')"
[[ "$ACTUAL_SHA_LOWER" == "$EXPECTED_SHA_LOWER" ]] || die "checksum verification failed; the downloaded archive was not installed."

# Refuse unexpected archive layouts before extraction. Release packages contain
# one versioned directory and the executable beneath it.
if ! tar -tzf "$ARCHIVE_PATH" | awk -v root="$PACKAGE/" '
  index($0, root) != 1 || $0 ~ /(^|\/)\.\.\// || $0 ~ /^\// { exit 1 }
  END { if (NR == 0) exit 1 }
'; then
  die "release archive has an unexpected layout; refusing to extract it."
fi
tar -xzf "$ARCHIVE_PATH" -C "$TEMP_DIR"
SOURCE_BINARY="$TEMP_DIR/$PACKAGE/t4e"
[[ -f "$SOURCE_BINARY" ]] || die "release archive does not contain the t4e executable."

mkdir -p -- "$PREFIX" || die "could not create install directory '$PREFIX'"
[[ -d "$PREFIX" && -w "$PREFIX" ]] || die "install directory '$PREFIX' is not writable; choose --prefix DIR."
TEMP_TARGET="$(mktemp "$PREFIX/.t4e.new.XXXXXX")" || die "could not prepare installation in '$PREFIX'"
cp "$SOURCE_BINARY" "$TEMP_TARGET"
chmod 755 "$TEMP_TARGET"
mv -f -- "$TEMP_TARGET" "$TARGET"

printf 'Installed T4E %s to %s\n' "$VERSION" "$TARGET"
case ":$PATH:" in
  *":$PREFIX:"*) ;;
  *)
    printf 'Add %s to your PATH, then open a new shell:\n  export PATH="%s:$PATH"\n' "$PREFIX" "$PREFIX"
    ;;
esac
printf 'The musl release is self-contained (no glibc dependency). Run: t4e\n'
if command -v tmux >/dev/null 2>&1; then
  tmux_version="$(tmux -V 2>/dev/null || true)"
  if [[ "$tmux_version" =~ tmux[[:space:]]+([0-9]+) ]] && (( BASH_REMATCH[1] >= 3 )); then
    printf 'tmux runtime detected: %s\n' "$tmux_version"
  else
    printf 'Warning: T4E requires tmux 3.x or newer; found %s. Upgrade tmux before running T4E.\n' "${tmux_version:-an unreadable version}" >&2
  fi
else
  printf 'Warning: T4E requires tmux 3.x or newer. Install tmux before running T4E.\n' >&2
fi
ai_provider=""
if command -v codex >/dev/null 2>&1 && codex login status >/dev/null 2>&1; then
  ai_provider="Codex"
elif command -v claude >/dev/null 2>&1 && claude auth status >/dev/null 2>&1; then
  ai_provider="Claude"
elif command -v gemini >/dev/null 2>&1 && {
  [ -n "${GEMINI_API_KEY:-}" ] || [ -n "${GOOGLE_API_KEY:-}" ] || [ -s "${HOME:-}/.gemini/oauth_creds.json" ]
}; then
  ai_provider="Gemini"
elif [ -n "${ZHIPU_API_KEY:-}" ]; then
  ai_provider="Zhipu AI"
elif [ -n "${MOONSHOT_API_KEY:-}" ]; then
  ai_provider="Kimi"
fi
if [ -n "$ai_provider" ]; then
  printf '%s AI provider credentials detected.\n' "$ai_provider"
else
  printf 'Notice: HOME AI stays disabled until a CLI or API provider is configured.\n' >&2
fi
