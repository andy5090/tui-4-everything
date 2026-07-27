#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"
export CI=1

ATTEMPT_TIMEOUT_SEC="${ATTEMPT_TIMEOUT_SEC:-600}"
MAX_ATTEMPTS="${MAX_ATTEMPTS:-2}"
INCONCLUSIVE_RERUNS="${INCONCLUSIVE_RERUNS:-1}"

pass() { printf '[PASS] %s\n' "$1"; }
fail() { printf '[FAIL] %s\n' "$1"; return 1; }

run_with_timeout() {
  local cmd="$1"
  local logfile="$2"
  if command -v timeout >/dev/null 2>&1; then
    timeout "${ATTEMPT_TIMEOUT_SEC}" bash -lc "$cmd" >"$logfile" 2>&1
  else
    bash -lc "$cmd" >"$logfile" 2>&1
  fi
}

is_inconclusive() {
  local logfile="$1"
  grep -Eqi '(timed out|timeout|temporary failure|network|dns|connection reset|5xx)' "$logfile"
}

run_with_policy() {
  local name="$1"
  local cmd="$2"
  local logfile="$3"

  local rerun=0
  while [[ "$rerun" -le "$INCONCLUSIVE_RERUNS" ]]; do
    local attempt=1
    local inconclusive=false

    while [[ "$attempt" -le "$MAX_ATTEMPTS" ]]; do
      if run_with_timeout "$cmd" "$logfile"; then
        pass "$name"
        return 0
      fi

      if is_inconclusive "$logfile"; then
        inconclusive=true
      fi
      attempt=$((attempt + 1))
    done

    if [[ "$inconclusive" == true && "$rerun" -lt "$INCONCLUSIVE_RERUNS" ]]; then
      rerun=$((rerun + 1))
      continue
    fi

    cat "$logfile"
    fail "$name"
    return 1
  done

  cat "$logfile"
  fail "$name"
  return 1
}

run_with_policy "cargo fmt" "cargo fmt --all -- --check" "/tmp/t4e-gates-fmt.log"
run_with_policy "cargo clippy" "cargo clippy --all-targets -- -D warnings" "/tmp/t4e-gates-clippy.log"
run_with_policy "cargo test" "cargo test --all-targets" "/tmp/t4e-gates-test.log"
run_with_policy "release installer" "tests/install_sh_test.sh" "/tmp/t4e-gates-install-sh.log"
run_with_policy "registry validation" "cargo run -- validate" "/tmp/t4e-gates-validate.log"
run_with_policy "workspace compiler" "cargo run -- workspace-plan --workspace-id video-desk --mux tmux" "/tmp/t4e-gates-workspace.log"

mkdir -p artifacts/contracts
run_with_policy "gate1 contract" "cargo run -- generate-contract-gate-report --gate-id gate1 --os contract-macos --output artifacts/contracts/gate1-report.json" "/tmp/t4e-gates-g1.log"
grep -q '"evidence_kind": "contract"' artifacts/contracts/gate1-report.json || fail "gate1-evidence"

run_with_policy "gate2 contract" "cargo run -- generate-contract-gate-report --gate-id gate2 --os contract-linux --output artifacts/contracts/gate2-report.json" "/tmp/t4e-gates-g2.log"
grep -q '"evidence_kind": "contract"' artifacts/contracts/gate2-report.json || fail "gate2-evidence"

run_with_policy "runtime gates 3-5" "scripts/gates/run_runtime_gates.sh artifacts/gates local-ci" "/tmp/t4e-gates-runtime.log"

echo "Gate protocol docs: tests/gates/gate{1..5}.md"
echo "Contract verification: PASS"
echo "Runtime Gates 3-5: PASS"
echo "Real OS install Gates 1-2: NOT RUN"
