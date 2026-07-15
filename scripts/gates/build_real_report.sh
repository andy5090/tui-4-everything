#!/usr/bin/env bash
set -euo pipefail

if [[ "$#" -ne 4 ]]; then
  echo "usage: $0 <gate1|gate2> <os-label> <results.json> <report.json>" >&2
  exit 2
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

gate_id="$1"
os_label="$2"
input="$3"
output="$4"

cargo run -- build-real-gate-report \
  --gate-id "$gate_id" \
  --os "$os_label" \
  --input "$input" \
  --output "$output"

grep -q '"evidence_kind": "real"' "$output"
grep -q '"status": "pass"' "$output"
