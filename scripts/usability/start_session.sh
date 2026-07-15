#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SESSION_ID="$(date -u +%Y%m%dT%H%M%SZ)"
SESSION_DIR="$ROOT_DIR/artifacts/usability/$SESSION_ID"
STATE_HOME="$SESSION_DIR/state"
BINARY="${1:-$ROOT_DIR/target/release/t4e}"

if [[ ! -x "$BINARY" ]]; then
  echo "release binary not found; run: cargo build --release --locked" >&2
  exit 1
fi

mkdir -p "$STATE_HOME"
{
  printf 'session_id=%s\n' "$SESSION_ID"
  printf 'binary=%s\n' "$BINARY"
  printf 'os=%s\n' "$(uname -srm)"
  printf 'terminal=%s\n' "${TERM:-unknown}"
  printf 'tmux=%s\n' "$(tmux -V 2>/dev/null || printf unavailable)"
  printf 'codex=%s\n' "$(codex --version 2>/dev/null || printf unavailable)"
} >"$SESSION_DIR/environment.txt"

printf '%s\n' \
  '# t4e usability session' \
  '' \
  '- Completed tasks:' \
  '- Failed or confusing tasks:' \
  '- Severe defects:' \
  '- Notes:' >"$SESSION_DIR/notes.md"

printf 'Session evidence: %s\n' "$SESSION_DIR"
printf 'Use docs/plans/usability-test.md as the task checklist.\n'
cd "$ROOT_DIR"
XDG_STATE_HOME="$STATE_HOME" "$BINARY"
printf 'Session state saved under: %s\n' "$SESSION_DIR"
