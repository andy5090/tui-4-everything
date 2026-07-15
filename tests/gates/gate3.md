# Gate 3 (tmux reproducibility)

- Compile tmux workspace plan uses `split-window -P -F "#{pane_id}"`
- Hash stability test must pass
- Pass condition: deterministic hash for equivalent snapshots
- `scripts/gates/run_runtime_gates.sh` must exercise real tmux and produce
  `artifacts/gates/gate3-report.json` with `evidence_kind: real` and hashed logs.
