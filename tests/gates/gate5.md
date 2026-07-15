# Gate 5 (agent safety policy)

- Agent tools default to `search_only`
- HIGH/script installs require explicit confirmation
- `scripts/gates/run_runtime_gates.sh` must produce
  `artifacts/gates/gate5-report.json` with `evidence_kind: real` and hashed logs.
