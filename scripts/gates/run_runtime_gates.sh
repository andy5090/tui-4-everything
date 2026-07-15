#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUTPUT_DIR="${1:-$ROOT_DIR/artifacts/gates}"
OS_LABEL="${2:-$(uname -s)-$(uname -m)}"

cd "$ROOT_DIR"
mkdir -p "$OUTPUT_DIR"

if ! command -v tmux >/dev/null 2>&1; then
  echo "tmux is required for Gate 3" >&2
  exit 1
fi

run_check() {
  local gate_id="$1"
  local check_id="$2"
  shift 2
  local logfile="$OUTPUT_DIR/${gate_id}-${check_id}.log"

  if "$@" >"$logfile" 2>&1; then
    printf '[PASS] %s/%s\n' "$gate_id" "$check_id"
  else
    cat "$logfile"
    printf '[FAIL] %s/%s\n' "$gate_id" "$check_id" >&2
    return 1
  fi
}

build_report() {
  local gate_id="$1"
  shift
  local args=()
  local check_id
  for check_id in "$@"; do
    args+=(--evidence "$check_id=$OUTPUT_DIR/${gate_id}-${check_id}.log")
  done
  cargo run --quiet -- build-runtime-gate-report \
    --gate-id "$gate_id" \
    --os "$OS_LABEL" \
    "${args[@]}" \
    --output "$OUTPUT_DIR/${gate_id}-report.json"
}

run_check gate3 tmux-live-repro cargo test --test tmux_runtime \
  three_registry_tmux_layouts_relaunch_with_matching_live_snapshots -- --exact --nocapture
run_check gate3 workspace-canonical-hash cargo test --test workspace_repro
build_report gate3 tmux-live-repro workspace-canonical-hash

run_check gate4 installer-execution cargo test --test install_execution
run_check gate4 queue-retry-state cargo test --test queue_state
build_report gate4 installer-execution queue-retry-state

run_check gate5 agent-policy cargo test --test contracts
run_check gate5 install-confirmation cargo test --test installer_logic
build_report gate5 agent-policy install-confirmation

printf 'Runtime release gates: PASS\n'
