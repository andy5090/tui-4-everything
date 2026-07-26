#!/usr/bin/env bash
set -euo pipefail

if [[ "$#" -ne 4 ]]; then
  echo "usage: $0 <binary> <output-dir> <version> <platform-label>" >&2
  exit 2
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="$1"
OUTPUT_DIR="$2"
VERSION="$3"
PLATFORM_LABEL="$4"
PACKAGE_NAME="t4e-${VERSION}-${PLATFORM_LABEL}"
STAGING_DIR="$(mktemp -d)"
trap 'rm -rf "$STAGING_DIR"' EXIT

test -x "$BINARY"
mkdir -p "$OUTPUT_DIR" "$STAGING_DIR/$PACKAGE_NAME/registry" "$STAGING_DIR/$PACKAGE_NAME/docs"
cp "$BINARY" "$STAGING_DIR/$PACKAGE_NAME/t4e"
cp "$ROOT_DIR/README.md" "$STAGING_DIR/$PACKAGE_NAME/README.md"
cp "$ROOT_DIR/CHANGELOG.md" "$STAGING_DIR/$PACKAGE_NAME/CHANGELOG.md"
cp "$ROOT_DIR/registry/catalog.yaml" "$STAGING_DIR/$PACKAGE_NAME/registry/catalog.yaml"
cp "$ROOT_DIR/registry/workspaces.yaml" "$STAGING_DIR/$PACKAGE_NAME/registry/workspaces.yaml"
cp "$ROOT_DIR/docs/architecture.md" "$STAGING_DIR/$PACKAGE_NAME/docs/architecture.md"

tar -C "$STAGING_DIR" -czf "$OUTPUT_DIR/$PACKAGE_NAME.tar.gz" "$PACKAGE_NAME"

if command -v sha256sum >/dev/null 2>&1; then
  sha256sum "$OUTPUT_DIR/$PACKAGE_NAME.tar.gz" >"$OUTPUT_DIR/$PACKAGE_NAME.tar.gz.sha256"
else
  shasum -a 256 "$OUTPUT_DIR/$PACKAGE_NAME.tar.gz" >"$OUTPUT_DIR/$PACKAGE_NAME.tar.gz.sha256"
fi

printf '%s\n' "$OUTPUT_DIR/$PACKAGE_NAME.tar.gz"
