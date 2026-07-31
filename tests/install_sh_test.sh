#!/usr/bin/env bash
# Network-free tests for install.sh. They use a mock curl/wget and local archives.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTALLER="$ROOT_DIR/install.sh"
TEST_DIR="$(mktemp -d "${TMPDIR:-/tmp}/t4e-install-test.XXXXXX")"
trap 'rm -rf "$TEST_DIR"' EXIT

fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }
assert_contains() { [[ "$1" == *"$2"* ]] || fail "expected '$2' in: $1"; }

write_sha256() {
  local file="$1" directory filename
  directory="$(dirname "$file")"
  filename="$(basename "$file")"
  (
    cd "$directory"
    if command -v sha256sum >/dev/null 2>&1; then
      sha256sum "$filename"
    else
      shasum -a 256 "$filename"
    fi
  ) >"$file.sha256"
}

verify_sha256() {
  local checksum="$1" directory filename
  directory="$(dirname "$checksum")"
  filename="$(basename "$checksum")"
  (
    cd "$directory"
    if command -v sha256sum >/dev/null 2>&1; then
      sha256sum --check "$filename"
    else
      shasum -a 256 --check "$filename"
    fi
  )
}

make_fixture() {
  local version="$1" arch="$2" package archive
  package="t4e-${version}-linux-${arch}-musl"
  mkdir -p "$TEST_DIR/fixtures/$package"
  printf '#!/bin/sh\necho t4e-%s\n' "$version" >"$TEST_DIR/fixtures/$package/t4e"
  chmod 755 "$TEST_DIR/fixtures/$package/t4e"
  archive="$TEST_DIR/fixtures/$package.tar.gz"
  tar -C "$TEST_DIR/fixtures" -czf "$archive" "$package"
  write_sha256 "$archive"
}

make_mock_downloader() {
  local name="$1"
  mkdir -p "$TEST_DIR/bin"
  cat >"$TEST_DIR/bin/$name" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
out=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --output|-o|--output-document) out="$2"; shift 2 ;;
    --output-document=*) out="${1#*=}"; shift ;;
    *) url="$1"; shift ;;
  esac
done
case "$url" in
  */releases/latest) printf '{"tag_name":"v9.9.9"}\n' >"$out" ;;
  *.tar.gz|*.tar.gz.sha256) cp "$T4E_TEST_FIXTURES/${url##*/}" "$out" ;;
  *) exit 42 ;;
esac
EOF
  chmod 755 "$TEST_DIR/bin/$name"
}

make_mock_uname() {
  cat >"$TEST_DIR/bin/uname" <<'EOF'
#!/usr/bin/env bash
if [[ "$1" == "-s" ]]; then echo Linux; else echo "${T4E_TEST_ARCH:-x86_64}"; fi
EOF
  chmod 755 "$TEST_DIR/bin/uname"
}

make_fixture 1.2.3 x86_64
make_fixture 1.2.3 aarch64
make_fixture 1.2.3 i686
make_fixture 9.9.9 x86_64
make_mock_downloader curl
make_mock_uname

prefix="$TEST_DIR/prefix"
output="$(PATH="$TEST_DIR/bin:$PATH" T4E_TEST_FIXTURES="$TEST_DIR/fixtures" "$INSTALLER" --version 1.2.3 --prefix "$prefix")"
[[ -x "$prefix/t4e" ]] || fail "x86_64 binary was not installed"
[[ "$("$prefix/t4e")" == "t4e-1.2.3" ]] || fail "installed binary is incorrect"
assert_contains "$output" "Installed T4E 1.2.3"
assert_contains "$output" "musl release is self-contained"

PATH="$TEST_DIR/bin:$PATH" T4E_TEST_FIXTURES="$TEST_DIR/fixtures" "$INSTALLER" --prefix "$TEST_DIR/latest-prefix" >/dev/null
[[ "$("$TEST_DIR/latest-prefix/t4e")" == "t4e-9.9.9" ]] || fail "latest release lookup is incorrect"

PATH="$TEST_DIR/bin:$PATH" T4E_TEST_FIXTURES="$TEST_DIR/fixtures" "$INSTALLER" --prefix "$prefix" --uninstall >/dev/null
[[ ! -e "$prefix/t4e" ]] || fail "uninstall did not remove target"

set +e
empty_prefix_output="$(PATH="$TEST_DIR/bin:$PATH" "$INSTALLER" --prefix '' --uninstall 2>&1)"
status=$?
set -e
[[ $status -ne 0 ]] || fail "an empty prefix unexpectedly succeeded"
assert_contains "$empty_prefix_output" "--prefix must be a non-empty directory"

# Force wget selection by hiding curl in a minimal PATH and provide essential tools.
rm "$TEST_DIR/bin/curl"
make_mock_downloader wget
checksum_tool="sha256sum"
command -v "$checksum_tool" >/dev/null 2>&1 || checksum_tool="shasum"
for tool in bash env cp chmod mkdir rm tar gzip "$checksum_tool" awk sed head mktemp mv tr; do
  ln -sf "$(command -v "$tool")" "$TEST_DIR/bin/$tool"
done
PATH="$TEST_DIR/bin" T4E_TEST_FIXTURES="$TEST_DIR/fixtures" T4E_TEST_ARCH=aarch64 "$INSTALLER" --version v1.2.3 --prefix "$TEST_DIR/aarch-prefix" >/dev/null
[[ "$("$TEST_DIR/aarch-prefix/t4e")" == "t4e-1.2.3" ]] || fail "aarch64 wget installation is incorrect"

# 32-bit x86 kernels commonly report i386 through i686. Every alias must
# resolve to the single i686 release artifact.
for reported_arch in i386 i486 i586 i686; do
  arch_prefix="$TEST_DIR/$reported_arch-prefix"
  PATH="$TEST_DIR/bin" T4E_TEST_FIXTURES="$TEST_DIR/fixtures" T4E_TEST_ARCH="$reported_arch" \
    "$INSTALLER" --version v1.2.3 --prefix "$arch_prefix" >/dev/null
  [[ "$("$arch_prefix/t4e")" == "t4e-1.2.3" ]] || fail "$reported_arch installation is incorrect"
done

# A well-formed but incorrect checksum must not overwrite an existing executable.
mkdir -p "$TEST_DIR/fail-prefix"
printf 'old binary\n' >"$TEST_DIR/fail-prefix/t4e"
printf '%064d  %s\n' 0 't4e-1.2.3-linux-x86_64-musl.tar.gz' \
  >"$TEST_DIR/fixtures/t4e-1.2.3-linux-x86_64-musl.tar.gz.sha256"
set +e
PATH="$TEST_DIR/bin:$PATH" T4E_TEST_FIXTURES="$TEST_DIR/fixtures" T4E_TEST_ARCH=x86_64 "$INSTALLER" --version 1.2.3 --prefix "$TEST_DIR/fail-prefix" >/dev/null 2>&1
status=$?
set -e
[[ $status -ne 0 ]] || fail "incorrect checksum unexpectedly succeeded"
[[ "$(cat "$TEST_DIR/fail-prefix/t4e")" == "old binary" ]] || fail "failed install overwrote existing executable"

# Release checksums must name the archive relative to the release directory so
# standard sha256sum verification succeeds after downloading both assets.
printf '#!/bin/sh\nexit 0\n' >"$TEST_DIR/package-t4e"
chmod 755 "$TEST_DIR/package-t4e"
"$ROOT_DIR/scripts/package_release.sh" "$TEST_DIR/package-t4e" "$TEST_DIR/package-output" 1.2.3 linux-x86_64-musl >/dev/null
verify_sha256 "$TEST_DIR/package-output/t4e-1.2.3-linux-x86_64-musl.tar.gz.sha256" \
  >/dev/null || fail "release checksum does not verify from its asset directory"

printf 'install.sh tests passed\n'
