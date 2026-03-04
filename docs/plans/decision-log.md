# t4e v0.1 Decision Log

## 2026-03-04

### D-001: Gate 1/2 sample set, scoring, retries, and report schema
- Decision:
  - Fix canonical starter sample size to `N=10` logical tool IDs:
    `curl`, `wget`, `jq`, `ripgrep`, `fzf`, `tmux`, `neovim`, `ffmpeg`, `yt-dlp`, `tree`.
  - Use `max_attempts_per_tool = 2` (1 initial + 1 retry).
  - Score as `success_rate = successful_tools / N`, where a tool is successful if any allowed attempt succeeds.
  - Always emit `first_pass_rate`, `retry_used_rate`, and `attempts_total`.
  - Require structured JSON report artifact under `artifacts/gates/gate{1|2}-report.json`.
- Why:
  - Removes ambiguity in denominator, retry handling, and pass/fail reproducibility.
- Impact:
  - Gate 1 threshold remains `>= 0.90`, Gate 2 remains `>= 0.60`, now with auditable metrics.

### D-002: zellij scope for v0.1
- Decision:
  - `tmux` is the only blocking mux target for v0.1 release gates.
  - `zellij` remains an explicit non-blocking stretch objective.
- Why:
  - Keeps v0.1 schedule risk bounded for a single engineer while retaining forward compatibility.
- Impact:
  - No release gate can fail solely due to missing zellij parity in v0.1.

### D-003: tmux reproducibility algorithm
- Decision:
  - Reproducibility hash is `SHA-256` over canonicalized records derived from tmux windows, panes, and command-injection logs.
  - Canonicalization includes whitespace normalization, workspace-root placeholder normalization, stable sorting, and explicit record formats (`W`, `P`, `C`).
- Why:
  - Provides deterministic and testable Gate 3 pass/fail criteria.
- Impact:
  - Gate 3 can be automated with direct hash equality checks across reruns.

### D-004: CI feasibility policy
- Decision:
  - Enforce noninteractive execution in CI (`CI=1`, noninteractive install flags, no prompt dependencies).
  - Set per-attempt timeout to `600s`, gate timeout to `2700s`.
  - Add failure classification as `infra` vs `product` at attempt and gate summary levels.
- Why:
  - Improves signal quality and reduces false-negative release decisions caused by external outages.
- Impact:
  - Gate reports can distinguish actionable product defects from transient CI environment issues.

### D-005: Minimal contract set for v0.1
- Decision:
  - Lock concrete minimal contracts for:
    - starter-pack schema
    - workspace template schema
    - risk rule mapping
    - screen event map
- Why:
  - Removes implementation ambiguity before coding starts.
- Impact:
  - Executor can begin work with stable interfaces and acceptance checks.

### D-006: Open questions resolution state
- Decision:
  - Both open items in `.omx/plans/open-questions.md` are resolved and marked closed.
- Why:
  - No remaining blocker-level ambiguity for v0.1 planning scope.
- Impact:
  - Planning state is ready for execution handoff.
