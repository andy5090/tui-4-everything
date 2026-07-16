# T4E v0.1 Completion Audit

Date: 2026-07-17

## Implemented And Proven Locally

| Requirement | Authoritative evidence | Result |
| --- | --- | --- |
| Engineering baseline | `scripts/gates/run_all.sh`, CI workflow, 99 default tests, Clippy warnings denied | Pass |
| Pack-first app navigation and responsive rendering | `tests/tui_state.rs`, real single-app tmux lifecycle, 120x30 release PTY smoke | Pass |
| Installation execution and recovery | `tests/install_execution.rs`, `tests/queue_state.rs`, `tests/storage_state.rs` | Pass |
| Full Linux catalog install and launch | Local package/source/dependency live gate plus manual `catalog-install-gate.yml`, isolated Ubuntu runner and evidence per app | Source and dependency gate pass; full install matrix not yet run |
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

## External Release Evidence

- Gate 1 passed on `macos-14`: 10/10 fixed-sample Homebrew installs succeeded
  on the first attempt, exceeding the 90% threshold.
- Gate 2 passed on `ubuntu-24.04`: 10/10 fixed-sample apt installs succeeded on
  the first attempt, exceeding the 60% threshold.
- Gates 3 through 5 passed with direct runtime logs and SHA-256 provenance.
- Linux x64 and macOS ARM64 release archives built, validated their embedded
  defaults outside the source tree, and published checksums.

The authoritative GitHub Actions run is
[`29430982685`](https://github.com/andy5090/tui-4-everything/actions/runs/29430982685),
executed for source SHA `932a5335c1b7f5ef820dff86651df7e027005826`.
Downloaded reports declare `evidence_kind: real` and `status: pass`; contract
reports are not used as release evidence.

## Next Stage

Automated self-verification, the first eight-task release-binary walkthrough,
and external Gates 1 through 5 are complete. All S0 through S2 findings are
closed; see `docs/plans/usability-results-2026-07-15.md`. Independent human
testing remains the final product-confidence step before the v0.1 release.

The full-catalog workflow is intentionally separate from the fast release gates.
It mutates a clean runner for every selected app and must pass before claiming
that every Registry installer is operational on Ubuntu 24.04.
