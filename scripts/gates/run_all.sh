#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

pass() { printf '[PASS] %s\n' "$1"; }
fail() { printf '[FAIL] %s\n' "$1"; return 1; }

cargo test >/tmp/t4e-gates-test.log 2>&1 || { cat /tmp/t4e-gates-test.log; fail "cargo test"; }
pass "cargo test"

cargo run -- validate >/tmp/t4e-gates-validate.log 2>&1 || { cat /tmp/t4e-gates-validate.log; fail "validate"; }
pass "registry validation"

cargo run -- workspace-plan --workspace-id video-desk --mux tmux >/tmp/t4e-gates-workspace.log 2>&1 || {
  cat /tmp/t4e-gates-workspace.log
  fail "workspace-plan"
}
pass "workspace compiler"

echo "Gate protocol docs: tests/gates/gate{1..5}.md"
echo "Overall: PASS"
