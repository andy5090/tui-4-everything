# t4e v0.1 Completion Audit

Date: 2026-07-15

## Implemented And Proven Locally

| Requirement | Authoritative evidence | Result |
| --- | --- | --- |
| Engineering baseline | `scripts/gates/run_all.sh`, CI workflow, 69 default tests, Clippy warnings denied | Pass |
| TUI navigation and responsive rendering | `tests/tui_state.rs`, 120x30 release PTY smoke | Pass |
| Installation execution and recovery | `tests/install_execution.rs`, `tests/queue_state.rs`, `tests/storage_state.rs` | Pass |
| tmux lifecycle and reproducibility | Gate 3 direct runtime report and hashed logs | Pass |
| Codex app-server control plane | Current-protocol test and signed-in streamed live turn | Pass |
| MCP protocol and fail-closed side effects | `tests/mcp_server.rs` and stdio lifecycle smoke | Pass |
| mpv, yazi, and newsboat adapters | `tests/app_adapters.rs` | Pass |
| Safety policy and typed approval | Gate 5 report, installer and TUI policy tests | Pass |
| Diagnostics and retry UX contracts | Gate 4 report and execution tests | Pass |
| Dependency security | RustSec scan of 153 locked dependencies | Pass, zero advisories |
| Packaging | Optimized Linux archive, SHA-256 verification, out-of-tree Registry validation | Pass |
| Release automation | `actionlint` 1.7.12 over CI and release workflows | Pass |

Generated local Gate 3 through 5 reports live under `artifacts/gates`. Each
report has `evidence_kind: real`, a successful Cargo test result for every
required check, and SHA-256 provenance for the direct log.

## External Evidence Still Required

- Gate 1 must run on a clean `macos-14` GitHub runner and meet the fixed
  ten-tool Homebrew threshold of 90%.
- Gate 2 must run on a clean `ubuntu-24.04` GitHub runner and meet the fixed
  ten-tool apt threshold of 60%.
- The macOS release archive must be built and validated by that workflow.

These checks are implemented in `.github/workflows/release-gates.yml`, but a
local Linux host cannot produce macOS evidence and this host does not have the
passwordless sudo required for an isolated apt gate. Contract reports are not
accepted as substitutes.

## Next Stage

Automated self-verification and the first eight-task release-binary walkthrough
are complete for the current host. All S0 through S2 findings are closed; see
`docs/plans/usability-results-2026-07-15.md`. Independent human testing and the
external Gate 1/2 runner evidence remain before v0.1 release.
