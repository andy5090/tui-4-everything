#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

restore_terminal() {
  if [[ -t 0 && -w /dev/tty ]]; then
    stty sane </dev/tty 2>/dev/null || true
    printf '\033[?1049l\033[?25h\033[0m' >/dev/tty 2>/dev/null || true
  fi
}
trap restore_terminal EXIT INT TERM HUP

if ! command -v cargo-watch >/dev/null 2>&1; then
  printf 'cargo-watch is required. Install it with:\n  cargo install cargo-watch --locked\n' >&2
  exit 1
fi

exec_status=0
cargo watch \
  --watch src \
  --watch registry \
  --watch Cargo.toml \
  --watch Cargo.lock \
  --exec 'run -- tui' || exec_status=$?

exit "$exec_status"
