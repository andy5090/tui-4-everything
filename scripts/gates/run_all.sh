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

mkdir -p artifacts/gates
cargo run -- generate-gate-report --gate-id gate1 --os macos-14 --output artifacts/gates/gate1-report.json >/tmp/t4e-gates-g1.log 2>&1 || {
  cat /tmp/t4e-gates-g1.log
  fail "gate1-report"
}
grep -q '"status": "pass"' artifacts/gates/gate1-report.json || fail "gate1-status"
pass "gate1 artifact"

cargo run -- generate-gate-report --gate-id gate2 --os ubuntu-24.04 --output artifacts/gates/gate2-report.json >/tmp/t4e-gates-g2.log 2>&1 || {
  cat /tmp/t4e-gates-g2.log
  fail "gate2-report"
}
grep -q '"status": "pass"' artifacts/gates/gate2-report.json || fail "gate2-status"
pass "gate2 artifact"

echo "Gate protocol docs: tests/gates/gate{1..5}.md"
echo "Overall: PASS"
