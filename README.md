# t4e

t4e is a curated terminal application manager, tmux workspace runtime, and
local Codex control surface. It uses the signed-in `codex` CLI account through
`codex app-server`; it does not request or proxy an OpenAI API key.

## Requirements

- Rust stable toolchain
- Linux or macOS
- tmux 3.x as the current hidden workspace process backend
- Codex CLI for AI Home
- The relevant package manager (`apt`, Snap, Homebrew, Cargo, or pipx) for installs

## Run

```bash
cargo run
```

For automatic development rebuilds and TUI restarts after source or Registry
changes:

```bash
scripts/dev-watch.sh
```

The script requires `cargo-watch` and restores the terminal screen and cursor
when it exits.

Main screens use `1` through `7`. Press `?` for navigation help.
Important actions are intentionally explicit:

- Catalog: `Enter` runs, `I` installs, and `U` uninstalls the current app.
- Install queue: `x` runs one item, `X` runs queued items sequentially.
- Workspaces: `Enter` starts and opens, `a` reopens, `x` stops all apps, `h`
  hashes a live snapshot, and `I` queues missing tools. The main flow is pack ->
  app -> run. In App View, `Tab`/`Shift-Tab` switches apps, `Alt+Backspace`
  returns to the previous screen while keeping apps running, and `Alt+Q` closes
  the current app. `Backspace` and `Esc` are forwarded to the running app.
  Text selection is the default mouse mode; `Alt+M` toggles t4e mouse controls
  for lists, app tabs, scrolling, and App View footer actions.
- App rows show install readiness and live install state. The detail panel keeps
  the current attempt, channel, recent package-manager output, and failure
  summary visible without opening the install utility.
- Pack screens list interactive apps only. One-shot commands such as `ffmpeg`,
  `jq`, and `ripgrep` remain available to pack installation, global search, and
  the AI control plane as support tools, but are not launched without inputs.
- Apps with registered flags open a launch-options dialog. Space enables an
  option, Left/Right chooses an allowlisted value, and Enter launches it.
- Package-manager installations can be removed with `U`; removal requires
  confirmation and verifies that the executable is gone.
  All other keys go to the current app; users do not need tmux commands.
- AI Home: `Enter` composes a request, `x` interrupts, and `A` reviews a
  proposed side effect.

Script, HIGH-risk, and AI-proposed side effects require an exact typed
confirmation. Codex runs in a read-only sandbox with app-server approvals set
to `never`; t4e remains authoritative for installation and process lifecycle.

On Linux, `x` or `X` handles the package-manager command without requiring the
user to type it. For apt, dnf, pacman, Snap, missing `pipx`, and declared Cargo
system dependencies, t4e temporarily leaves the alternate screen and runs
interactive `sudo -v`; after authentication it returns to the TUI and executes
the approved install noninteractively. Install processes are serialized, apt
waits for an existing dpkg lock, and Cargo apps receive one 30-minute build
attempt. Cancelling the sudo prompt leaves the queue item unexecuted.

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

The separate `Full catalog install verification` workflow is a manual,
destructive integration gate for Linux. It derives its matrix from the Registry,
gives every app a fresh `ubuntu-24.04` runner, runs the real installer, verifies
the executable path, performs a bounded launch smoke, and uploads per-app install
and launch evidence. Run it from GitHub Actions with either the `starter` or
`all` exposure. The local plan can be audited without installing anything:

```bash
cargo run -- catalog-plans --platform linux --exposure starter
```

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

- tmux is the hidden workspace process backend. App View owns normal launch,
  display, keyboard input, switching, and close controls. Zellij layouts
  compile, but App View lifecycle parity is not implemented yet.
- Native adapters are provided for mpv JSON IPC, yazi, and newsboat. The latter
  two verify the tmux pane process and accept only application-specific keys.
- App View input is limited to pane IDs discovered inside t4e-managed sessions;
  commands are passed as structured tmux arguments without shell evaluation.
