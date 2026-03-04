# Deep-Interview Execution Spec: t4e Spec v0.1

## Metadata
- Profile: standard
- Rounds completed: 1
- Final ambiguity: 0.183
- Threshold: <= 0.20
- Context type: greenfield
- Context snapshot: .omx/context/t4e-spec-v0-1-20260304T044921Z.md
- Transcript: latest file under .omx/interviews/t4e-spec-v0-1-*.md

## Clarity Breakdown
| Dimension | Score | Notes |
|---|---:|---|
| Goal Clarity | 0.82 | Product direction and v0.1 scope are clear. |
| Constraint Clarity | 0.84 | v0.1 mux/safety/agent boundaries are explicit. |
| Success Criteria Clarity | 0.79 | Release gating tests defined with measurable thresholds. |

## Goal
Deliver t4e v0.1 as a terminal-first dashboard that lets users discover, install, and run curated tools and launch tmux/zellij workspaces, with starter-first UX and optional search-only agent tooling.

## Constraints
- v0.1 supports external mux backend only (`tmux`/`zellij`), not built-in PTY split.
- Starter packs emphasize general user value (entertainment/files/fun/edit).
- Agents remain `search_only` by default, with `HIGH` risk warning UX.
- Script-based installers must require explicit confirmation plus command preview.
- Package-manager-first installation strategy with resolver fallback is mandatory.

## Non-goals
- No built-in terminal multiplexer in v0.1.
- No hard enforcement of agent internal file/command restrictions in v0.1.
- No advanced recommendation/personalization system in v0.1.
- No remote registry signing/verification beyond basic mechanisms in v0.1.

## Testable Acceptance Criteria (Release Gates)
1. Starter install success (macOS)
- Scope: Starter packs installation on macOS using brew-first strategy.
- Pass: overall tool installation success rate >= 90%.

2. Linux resolver baseline
- Scope: apt-family distro with package hint + search resolver behavior.
- Pass: Starter-target installation success rate >= 60%.

3. tmux workspace reproducibility
- Scope: at least 3 tmux workspaces (e.g., Video/Music/Fun Desk).
- Pass: each launches with intended split layout and command injection reproducibly.

4. Failure diagnostics and retry UX
- Scope: induced install failure paths.
- Pass: exit code + stderr summary + full log persistence + retry path available.

5. Agent safety/visibility policy
- Scope: Claude/Codex/OpenCode listing and install flow.
- Pass: agents default to search-only exposure and display `HIGH` risk warning with required confirmation flow for risky/script installs.

## Assumptions Exposed and Resolutions
- Assumption: Existing candidate acceptance tests were directionally correct.
- Resolution: confirmed by user, with stricter macOS overall install target raised from 80% to 90%.

- Assumption: Greenfield implementation planning is still needed before coding.
- Resolution: yes; handoff should go to planning/execution orchestration.

## Technical Context Findings
- Repository currently contains no source implementation files in scope; only `.omx` state/log artifacts exist.
- This is treated as greenfield from implementation perspective.

## Transcript (Condensed)
- Round 1 target: Success Criteria Clarity
- Question: finalize 5 prioritized acceptance tests with pass metrics
- User answer: keep proposed set; raise overall tool install success target to 90%
- Result: ambiguity reduced to 0.183 (threshold met)
