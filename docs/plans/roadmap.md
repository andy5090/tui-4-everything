# t4e Product Roadmap

## Product Direction

t4e starts as a curated TUI application manager and workspace launcher, then
grows into an AI-assisted terminal environment. Codex is the control plane that
interprets intent and plans multi-application work. t4e remains the trusted
runtime that owns permissions, process lifecycle, tmux/zellij sessions, logs,
and persistent state.

The AI layer must use bounded, structured t4e actions. Generic terminal-screen
reading and synthetic key input are a later fallback, not the primary control
mechanism.

## Delivery Sequence

## Current Implementation Status

- Phase 0 engineering baseline and CI: complete.
- Phase 1 basic TUI shell: complete.
- Phase 2 command runner, timeout, retry, cancellation, streamed output,
  diagnostics, install logs, queue persistence, single-tool and sequential pack
  execution, preflight/postflight checks, favorites, recents, and settings:
  complete.
- Phase 3 tmux lifecycle, preflight, attach/stop, live snapshots, restoration,
  and reproducibility verification: complete.
- Phase 4 Codex app-server client, AI Home, bounded intent actions, typed
  approval, usage events, and t4e MCP server: complete.
- Phase 5 mpv, yazi, and newsboat adapters with allowlists, observation,
  compensating actions, and audit records: complete.
- Phase 6 packaging, protocol checks, CI, and real-gate workflow: complete.
  Gates 1 through 5 passed on isolated GitHub runners, and Linux x64 plus macOS
  ARM64 release packages passed out-of-tree validation.
- Pack-first app shell: implemented. Selecting a pack opens its apps, Enter
  launches the selected app, missing apps install and then launch, and the
  embedded App View owns switching and process lifecycle without exposing tmux
  controls.
- Ubuntu catalog hardening: package-source candidates and declared build
  dependencies are live-checked; installs are serialized; apt lock contention,
  missing pipx, Cargo build duration, and multi-binary apps are handled.

### Phase 0: Engineering Baseline

- Apply and enforce rustfmt, Clippy, and tests in CI.
- Separate contract-only gate reports from real OS installation gates.
- Strengthen catalog and workspace reference validation.
- Replace placeholder installer URLs and verify package hints.

Exit criteria:

- Pull requests run formatting, lint, and unit/integration tests.
- Mock gate results cannot be presented as real installation success.

### Phase 1: Pack-First TUI Shell

- Make packs the main screen and open a pack directly into its app list.
- Launch the selected app with Enter; keep installs, workspaces, AI, logs, and
  settings as secondary utilities rather than top-level tabs.
- Show install state, attempts, recent output, and failures directly in the app
  list and detail panel while an app is being prepared for launch.
- Switch running apps with Tab/Shift-Tab, return with Alt+Backspace while keeping
  the app alive, and explicitly terminate the selected app with Alt+Q.
- Keep terminal text selection as the default mouse mode and toggle interactive
  t4e mouse capture with Alt+M.
- Expose allowlisted launch flags/arguments and verified package-manager
  uninstall actions in the app selection screen.
- Support keyboard navigation, catalog search, list selection, and help.
- Display registry-backed packs, tools, risk levels, and workspace layouts.
- Keep one-shot support commands available to pack installs and AI while only
  presenting input-ready interactive apps in each pack launcher.
- Keep all existing bootstrap CLI commands available.

Exit criteria:

- Running `t4e` opens the dashboard and restores the terminal on exit.
- A user can navigate every screen without editing configuration files.
- Narrow terminals degrade to a usable single-column layout.

### Phase 2: Headless Execution Core

- Add an injectable command runner and real package-manager execution.
- Connect the queue state machine to workers, timeout, retry, and cancellation.
- Stream stdout/stderr into diagnostics and durable logs.
- Persist installed tools, favorites, recents, and settings as JSON.
- Add preflight and post-install checks.

Exit criteria:

- A single tool and a selected pack can be installed from CLI and TUI.
- Failed operations expose an error summary, full log, and retry action.

### Phase 3: Workspace Runtime

- Launch, attach, inspect, and stop tmux workspaces.
- Validate required tools before launch and offer a bounded install plan.
- Capture live tmux snapshots and verify reproducibility hashes.
- Keep zellij parity non-blocking until its integration suite is available.

Exit criteria:

- Video, Music, and Fun workspaces launch twice with matching snapshots.
- t4e can return to, switch between, and stop managed sessions.

### Phase 4: Codex Control Plane

- Integrate `codex app-server` over local stdio JSON-RPC.
- Reuse each user's own Codex login; never copy or proxy Codex credentials.
- Stream thread, turn, item, approval, and usage events into an AI Home screen.
- Expose t4e as an MCP server with structured tools such as catalog search,
  install planning, workspace launch, app start, app observation, pane switch,
  and app stop.
- Keep deterministic navigation local; call Codex for ambiguous intent,
  multi-application planning, diagnosis, and recovery.

Exit criteria:

- Natural-language requests can search the catalog and launch a workspace.
- Side-effecting actions pass through the t4e policy engine and user approval.
- AI work survives screen changes and can be interrupted or resumed.

### Phase 5: Application Adapters

- Prefer native control surfaces such as mpv JSON IPC and tmux commands.
- Define per-application observe/action schemas and capability declarations.
- Add audit history and undo or compensating actions where possible.
- Start with three reliable adapters before expanding the catalog.

Exit criteria:

- Supported apps report structured state and accept validated actions.
- Unsupported actions fail closed without falling back to arbitrary shell.

### Phase 6: Release Hardening

- Run real macOS Homebrew and Ubuntu apt installation gates.
- Package t4e and document supported systems, permissions, and limitations.
- Add crash recovery, session restoration, and compatibility checks for the
  installed Codex app-server protocol version.

Exit criteria:

- macOS reaches the 90% target and Ubuntu reaches the 60% target on the fixed
  ten-tool sample set.
- All five release gates use real artifacts and block releases correctly.

## v0.2 Experiments

- Generic PTY screen observation with ANSI normalization: implemented for
  t4e-managed tmux panes.
- Bounded synthetic key input for applications without an adapter: implemented
  for explicitly selected, t4e-managed panes; expand the per-app key policy.
- Per-application reliability evaluation before enabling autonomous control.
- Voice or remote clients only after local authentication and policy boundaries
  have been proven.

## Verification Snapshot (2026-07-16)

- Formatting, Clippy with warnings denied, unit/integration tests, Registry
  validation, MCP contracts, and contract gates pass locally. The suite has 69
  passing default tests and two intentionally explicit live-plan tests.
- Codex CLI 0.144.4 app-server initialization, ChatGPT account discovery, and
  one signed-in structured live turn pass.
- Video, Music, and Fun tmux layouts each launch and stop twice with matching
  live reproducibility hashes using non-networked test pane commands.
- A 120x30 PTY smoke test renders AI Home with the signed-in account, returns
  Home, exits cleanly, persists state, and leaves no test sessions.
- RustSec `cargo-audit` scans all 153 locked dependencies with zero
  vulnerabilities or warnings after the Ratatui, Crossterm, and anyhow
  dependency updates. CI repeats the audit with `rustsec/audit-check`.
- Gates 3, 4, and 5 produce direct-runtime `real/pass` reports with a SHA-256
  for every test log. The Linux release archive builds, verifies its checksum,
  and validates its embedded Registry outside the source tree.
- GitHub Actions run
  [`29430982685`](https://github.com/andy5090/tui-4-everything/actions/runs/29430982685)
  passed the real macOS Homebrew and Ubuntu apt gates at 10/10 first-attempt
  installs, then built and validated Linux x64 and macOS ARM64 archives.
- A task-based, isolated hands-on protocol is ready at
  `docs/plans/usability-test.md`. The first release-binary walkthrough completed
  all eight tasks after closing one S1, four S2, and two S3 findings; results
  are in `docs/plans/usability-results-2026-07-15.md`.

## Safety Boundaries

- t4e, not the model, is authoritative for process and permission decisions.
- Observe, navigate, execute, install, destructive, network, and secret access
  are separate capabilities.
- Install, delete, external transmission, and secret access require explicit
  approval and an audit record.
- App-server stays local by default. Any remote transport requires TLS and
  authenticated capability tokens.
