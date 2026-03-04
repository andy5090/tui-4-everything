# Gate 3 (tmux reproducibility)

- Compile tmux workspace plan uses `split-window -P -F "#{pane_id}"`
- Hash stability test must pass
- Pass condition: deterministic hash for equivalent snapshots
