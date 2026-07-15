# Gate 4 (diagnostics + retry UX)

- Failure diagnostics include exit code + stderr summary + full log path
- Queue state transitions support retryable failed item
- `scripts/gates/run_runtime_gates.sh` must produce
  `artifacts/gates/gate4-report.json` with `evidence_kind: real` and hashed logs.
