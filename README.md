# t4e

t4e is a curated terminal application manager, tmux workspace runtime, and
local Codex control surface. It uses the signed-in `codex` CLI account through
`codex app-server`; it does not request or proxy an OpenAI API key.

## Requirements

- Rust stable toolchain
- Linux or macOS
- tmux 3.x for workspace launch and attach
- Codex CLI for AI Home
- The relevant package manager (`apt` or Homebrew) for installs

## Run

```bash
cargo run
```

Main screens use `1` through `7` or `Tab`. Press `?` for navigation help.
Important actions are intentionally explicit:

- Catalog: `Space` selects, `I` queues selected tools.
- Install queue: `x` runs one item, `X` runs queued items sequentially.
- Workspaces: `Enter` launches, `a` attaches, `x` stops, `h` hashes a live
  snapshot, and `I` queues missing tools.
- AI Home: `Enter` composes a request, `x` interrupts, and `A` reviews a
  proposed side effect.

Script, HIGH-risk, and AI-proposed side effects require an exact typed
confirmation. Codex runs in a read-only sandbox with app-server approvals set
to `never`; t4e remains authoritative for installation and process lifecycle.

## CLI

```bash
cargo run -- validate
cargo run -- install-plan --tool-id ripgrep --platform linux
cargo run -- install --tool-id ripgrep --yes
cargo run -- workspace-plan --workspace-id video-desk --mux tmux
cargo run -- mcp-server
```

The MCP stdio server implements protocol revision `2025-06-18`. Discovery and
planning tools are read-only. Side-effect tools fail closed until an approval
is granted in the TUI.

## Verification

```bash
scripts/gates/run_all.sh
```

This runs formatting, Clippy, all default tests, Registry validation, workspace
compilation, Gate 1/2 contract reports, and direct-runtime Gates 3 through 5.
Contract reports are written under `artifacts/contracts` and never count as
release evidence. Runtime reports and their hashed logs are written under
`artifacts/gates`.

CI additionally runs the official RustSec `audit-check` action against
`Cargo.lock`. For local dependency auditing, install `cargo-audit` and run
`cargo audit`.

The manual `Real release gates` GitHub workflow installs the fixed ten-tool
sample on clean `macos-14` and `ubuntu-24.04` runners. It also produces direct
runtime evidence for Gates 3 through 5, then packages Linux and macOS binaries
only after all five gates pass. Every report contains source SHA-256 evidence.

Default catalog and workspace registries are embedded in the binary, so the
packaged executable works outside the source tree. Release archives also ship
editable Registry copies, the README, architecture notes, and a SHA-256 file.

The signed-in Codex live-turn test is intentionally excluded from normal CI:

```bash
cargo test --test codex_live_turn -- --ignored --nocapture
```

After automated verification, start the hands-on usability protocol with:

```bash
scripts/usability/start_session.sh
```

The task list and acceptance criteria are in
`docs/plans/usability-test.md`.

If the account's configured model requires a newer Codex CLI, t4e retries once
with `gpt-5.4`. Set `T4E_CODEX_FALLBACK_MODEL` to select another compatible
model available through the signed-in plan. t4e never runs `codex update`
automatically.

## State And Recovery

User state defaults to `$XDG_STATE_HOME/t4e/state.json` or
`~/.local/state/t4e/state.json`. Writes are atomic. Interrupted installs are
restored as failed and retryable; managed tmux sessions are rediscovered on
startup; install logs are stored beside the state file.

## Current Boundaries

- tmux is the supported workspace runtime. Zellij layouts compile, but live
  lifecycle parity is not a release blocker yet.
- Native adapters are provided for mpv JSON IPC, yazi, and newsboat. The latter
  two verify the tmux pane process and accept only application-specific keys.
- Generic PTY observation and arbitrary synthetic key input remain disabled.
