# t4e v0.1 Consensus Plan (Revised)

## Context
- Source inputs:
  - `.omx/specs/deep-interview-t4e-spec-v0-1.md`
  - `.omx/plans/t4e-v0.1-implementation-plan.md`
  - Mandatory review items (Gate math/schema, zellij scope, tmux reproducibility hash, CI feasibility constraints, concrete contracts)
- Project state: greenfield Rust TUI; planning artifacts exist, implementation not started.
- Plan mode: consensus, `SHORT` (not deliberate).

## RALPLAN-DR Summary

### Principles
1. Keep v0.1 shippable for one engineer with strict scope control.
2. Prefer deterministic, auditable gate math over optimistic heuristics.
3. Treat reproducibility as data contract + hashing algorithm, not prose intent.
4. Separate product defects from CI infrastructure instability.
5. Add only minimal contracts required to unblock execution and testing.

### Top Decision Drivers
1. Release gate pass/fail must be reproducible across runs and environments.
2. v0.1 timeline risk must stay bounded (no multiplexor parity rewrite).
3. CI signal quality must distinguish flaky infrastructure from true regressions.

### Options Considered

#### Option A: tmux-only v0.1, zellij dropped entirely
- Pros: lowest delivery risk, simplest validation matrix.
- Cons: diverges from original product direction that mentions tmux/zellij support.

#### Option B (Chosen): tmux blocking scope + zellij explicit non-blocking stretch
- Pros: preserves v0.1 delivery certainty while keeping forward path for zellij.
- Cons: partial mux capability in v0.1 (documented limitation).

#### Option C: tmux + zellij both blocking in v0.1
- Pros: stronger cross-mux parity in first release.
- Cons: high schedule risk and larger CI/environment matrix for a greenfield app.

## ADR
- Decision:
  - Use a deterministic gate contract with fixed sample sets, explicit scoring formulas, retry accounting, and JSON report schema.
  - Make `tmux` the only blocking mux target for v0.1.
  - Keep `zellij` as a non-blocking stretch objective.
- Drivers:
  - Reproducibility and auditable quality gates.
  - Single-engineer delivery constraints.
  - CI feasibility without interactive steps.
- Alternatives considered:
  - tmux-only with no zellij path (rejected: product direction mismatch).
  - tmux+zellij both blocking (rejected: schedule and CI risk too high).
- Why chosen:
  - Gives clear release control now, without closing future zellij support.
- Consequences:
  - v0.1 gate matrix is smaller and more stable.
  - zellij outcomes are tracked but do not block release.
- Follow-ups:
  - Promote zellij from stretch to blocker only in v0.2 planning when parity test budget is available.

## Work Objectives
1. Preserve the 5 v0.1 release gates, but make all gate rules machine-checkable.
2. Lock down tmux reproducibility verification with a canonical hash algorithm.
3. Define minimal data contracts required by installer, workspace, safety, and UI event routing.
4. Keep zellij visible but non-blocking for v0.1 release readiness.

## Guardrails

### Must Have
- Gate 1/2 fixed sample set definition and explicit success formulas.
- Retry counting rules and report schema for Gate 1/2 output artifacts.
- Deterministic tmux canonicalization + SHA-256 reproducibility hash for Gate 3.
- CI noninteractive policy, timeout/retry policy, and failure classification (`infra` vs `product`).
- Concrete minimal contracts for starter packs, workspace templates, risk rules, and screen events.

### Must NOT Have (v0.1)
- No release blocking on zellij parity.
- No interactive prompts in CI (sudo password, TTY confirmations, manual approvals).
- No gate pass/fail logic based on free-form log interpretation.

## Gate Contract Revisions

### 1) Gate 1/2 Canonical Sample Set
- Canonical sample size: `N = 10` unique tool IDs.
- Canonical tool IDs (shared logical set for Gate 1 and Gate 2):
  - `curl`, `wget`, `jq`, `ripgrep`, `fzf`, `tmux`, `neovim`, `ffmpeg`, `yt-dlp`, `tree`
- Rule: Gate 1 and Gate 2 must use this exact logical sample list (package-manager names are mapped per OS through schema hints).
- Rule: Denominator is always `N` unique tool IDs, never number of attempts.

### 2) Gate 1/2 Scoring Formula + Retry Counting
- Retry budget: `max_attempts_per_tool = 2` (initial attempt + 1 retry).
- Per-tool outcome:
  - `success` if any attempt succeeds within attempt budget.
  - `failure` otherwise.
- Formulas:
  - `success_rate = successful_tools / N`
  - `first_pass_rate = first_attempt_successes / N`
  - `retry_used_rate = tools_with_retry / N`
  - `attempts_total = sum(tool_attempt_count)`
- Pass thresholds:
  - Gate 1 (macOS): `success_rate >= 0.90`
  - Gate 2 (Linux): `success_rate >= 0.60`
- Reporting rule: retries can improve `success_rate`, but `first_pass_rate` and `retry_used_rate` must always be reported for transparency.

### 3) Gate 1/2 Report Schema (JSON Contract)
Gate output file: `artifacts/gates/gate{1|2}-report.json`

```json
{
  "gate_id": "gate1",
  "run_id": "20260304T130000Z",
  "os": "macos-14",
  "sample_set": {
    "version": "v0.1",
    "size": 10,
    "tool_ids": ["curl", "wget", "jq", "ripgrep", "fzf", "tmux", "neovim", "ffmpeg", "yt-dlp", "tree"]
  },
  "policy": {
    "max_attempts_per_tool": 2,
    "per_attempt_timeout_sec": 600,
    "gate_timeout_sec": 2700
  },
  "summary": {
    "first_attempt_successes": 8,
    "successful_tools": 9,
    "tools_with_retry": 3,
    "attempts_total": 13,
    "first_pass_rate": 0.8,
    "success_rate": 0.9,
    "retry_used_rate": 0.3,
    "infra_failures": 1,
    "product_failures": 0,
    "status": "pass"
  },
  "tool_results": [
    {
      "tool_id": "ripgrep",
      "manager": "brew",
      "attempt_count": 2,
      "final_status": "success",
      "failure_classification": "infra",
      "attempts": [
        { "attempt": 1, "exit_code": 1, "duration_ms": 40123, "stderr_summary": "network timeout" },
        { "attempt": 2, "exit_code": 0, "duration_ms": 12211, "stderr_summary": "" }
      ]
    }
  ]
}
```

### 4) Gate 3 tmux Reproducibility: Canonicalization + Hash Algorithm
- Inputs:
  - `tmux list-windows -F "#{window_index}\t#{window_name}\t#{window_layout}"`
  - `tmux list-panes -F "#{window_index}\t#{pane_index}\t#{pane_width}\t#{pane_height}\t#{pane_start_command}"`
  - Command-injection log captured at launch (`window_index`, `pane_index`, `sequence`, `command`).
- Canonicalization steps:
  1. Trim whitespace and normalize internal whitespace to single spaces.
  2. Replace absolute workspace root prefix with `$WORKSPACE_ROOT`.
  3. Normalize empty commands to `<none>`.
  4. Build canonical records:
     - `W|{window_index}|{window_name}|{window_layout}`
     - `P|{window_index}|{pane_index}|{pane_width}x{pane_height}|{pane_start_command}`
     - `C|{window_index}|{pane_index}|{sequence}|{command}`
  5. Sort `W` and `P` by numeric indices; sort `C` by numeric `sequence`.
  6. Join with `\n` using UTF-8.
- Hash algorithm:
  - `repro_hash = SHA256(canonical_text)`
- Gate 3 pass rule:
  - For each template (`video`, `music`, `fun-desk`), two consecutive launches must produce identical `repro_hash`.

### 5) CI Feasibility Constraints
- Noninteractive policy:
  - CI jobs must run with `CI=1`.
  - Commands must use noninteractive flags where available (`--yes`, `-y`, `DEBIAN_FRONTEND=noninteractive`).
  - Any prompt-required command is an immediate `product` failure for gate scripts.
- Timeout/retry policy:
  - Per tool install attempt timeout: `600s`.
  - Gate job timeout: `2700s` (45m).
  - Automatic retry: one extra attempt per tool (already encoded in Gate 1/2 policy).
  - Optional gate rerun once if run status is `inconclusive` due to infrastructure outage.
- Failure classification:
  - `infra` examples: DNS/network timeout, package index 5xx, runner disk exhaustion, host-level apt/brew outage.
  - `product` examples: invalid resolver mapping, malformed command generation, missing confirmation handling, parser/runtime panic.
  - Classification result must be emitted per failed attempt and summarized at gate level.

## Minimal Contract Definitions

### A) Starter-Pack Schema (`assets/starter-packs/*.toml`)
```toml
id = "fun-pack"
title = "Fun Pack"
version = "0.1.0"
description = "General starter pack"

[[tools]]
id = "ripgrep"
display_name = "ripgrep"
fallback_query = "ripgrep package"
risk_tags = ["network"]

[tools.manager_hints]
brew = "ripgrep"
apt = "ripgrep"
```

Required fields:
- pack: `id`, `title`, `version`, `description`, `tools[]`
- tool: `id`, `display_name`, `fallback_query`, `manager_hints.{brew|apt optional at least one}`, `risk_tags[]`

### B) Workspace Template Schema (`assets/workspaces/*.toml`)
```toml
id = "video"
title = "Video Desk"
default_mux = "tmux"

[[windows]]
name = "main"
layout = "even-horizontal"

[[windows.panes]]
cwd = "$WORKSPACE_ROOT"
startup_command = "nvim"
```

Required fields:
- template: `id`, `title`, `default_mux`, `windows[]`
- window: `name`, `layout`, `panes[]`
- pane: `cwd`, `startup_command`

### C) Risk Rule Mapping (`src/agents/risk.rs` contract)
| Signal | Risk Level | UX Requirement |
|---|---|---|
| package-manager install + no risky tags | LOW | no extra confirmation |
| package-manager install + `risk_tags` contains `system`/`privileged` | MEDIUM | warning banner + single confirm |
| script install (`curl|wget` piped to shell, direct script URL, `sh -c`) | install policy | command preview + explicit typed confirmation |
| agent tools (`Claude`, `Codex`, `OpenCode`) in v0.1 | COMMANDS + AUTONOMOUS = DANGER | include in starter; require explicit approval |

### D) Screen Event Map (`src/app/events.rs` and screen reducers)
| Screen | Key/Event | Action |
|---|---|---|
| `Home` | `c` | go to Catalog |
| `Home` | `i` | go to Install Queue |
| `Home` | `w` | go to Workspaces |
| `Home` | `a` | go to Agents |
| `Home` | `l` | go to Logs |
| `Catalog` | `j`/`k` | move selection |
| `Catalog` | `Enter` | open starter-pack detail |
| `Catalog` | `I` | queue install for selected pack |
| `Install` | `r` | retry selected failed item |
| `Workspace` | `Enter` | launch selected template |
| `Workspace` | `h` | show reproducibility hash detail |
| `Agents` | `Enter` | open risk/details (no execute) |
| `Logs` | `/` | filter logs |
| `*` | `q` | back/quit according to stack depth |

## Task Flow (Actionable 5-Step Plan)

1. Finalize gate contracts and schema artifacts.
Acceptance criteria:
- Gate 1/2 formula, retry policy, and JSON schema are documented and internally consistent.
- Canonical sample set (`N=10`) is fixed and referenced in one source-of-truth section.

2. Re-scope mux delivery for v0.1.
Acceptance criteria:
- tmux is the only blocking mux target in v0.1 success criteria.
- zellij is labeled as non-blocking stretch with explicit non-gating status.

3. Define deterministic tmux reproducibility hashing.
Acceptance criteria:
- Canonicalization inputs, transformation rules, sorting, and SHA-256 hashing steps are specified.
- Gate 3 pass/fail rule references identical hash comparison across consecutive launches.

4. Define CI feasibility and classification policy.
Acceptance criteria:
- Noninteractive requirements and timeout/retry budgets are explicit.
- `infra` vs `product` classification rules are listed with examples and report implications.

5. Lock minimal contracts for execution handoff.
Acceptance criteria:
- Starter-pack schema, workspace template schema, risk mapping, and screen event map are all concrete and minimally sufficient.
- Plan is ready for executor implementation without unresolved blocking ambiguity.

## Success Criteria
- Review items (1) through (5) are fully addressed with concrete rules/contracts.
- `open-questions.md` entries tied to this plan are closed with explicit decisions.
- Plan is executable by an implementation agent without further scope clarifications for v0.1.
