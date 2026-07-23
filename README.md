# T4E

T4E is a curated terminal application manager, tmux workspace runtime, and
local Codex control surface. It uses the signed-in `codex` CLI account through
`codex app-server`; it does not request or proxy an OpenAI API key.

## Requirements

- Rust stable toolchain
- Linux or macOS
- tmux 3.x as the current hidden workspace process backend
- Codex CLI for AI Home
- The relevant package manager (`apt`, Snap, Homebrew, Cargo, or pipx) for installs

## Catalog Packs

| Pack | Interactive apps |
| --- | --- |
| Music | Spotatui, Spotify Player, Ncspot, Cava, Termusic |
| Video | Yewtube, YouTube TUI, tplay |
| Podcasts & News | Shellcast, Newsboat |
| Information Search | Lynx |
| Viewers | Fastfetch, Glow (with bat, less, and MediaInfo support tools) |
| Files | Yazi, ncdu, broot |
| Fun | cmatrix, Asciiquarium, tty-clock, nyancat, pipes.sh, and visual utilities |
| Games | bastet, ninvaders, nudoku |
| Editors & IDEs | Micro, Helix, LazyVim, VisiData |
| Agents | Claude Code, Codex CLI, OpenCode |

Support commands such as `mpv`, `yt-dlp`, `ffmpeg`, `jq`, and `ripgrep` are
installed as pack dependencies but are not launched without required input.
LazyVim uses an isolated `t4e-lazyvim` profile, leaving an existing Neovim
configuration untouched. YouTube TUI provides browsing and search; tplay asks
for a media URL or local path before rendering the media as terminal ASCII.
On Linux, both media paths use T4E-managed current `yt-dlp` environments rather
than the frequently outdated Ubuntu repository build. T4E also configures
Yewtube to use `mpv` when its saved player is missing or unavailable, while
preserving any working custom player.

Catalog exposure is internal release metadata: `starter` participates in the
default curated release checks and `labs` is experimental. Every current pack,
including Agents, is `starter`. Exposure is hidden from the TUI because it is
not an installation or permission decision.

Each app declares any combination of `NETWORK`, `ACCOUNT`, `FILE_READ`,
`FILE_WRITE`, `DELETE`, `SYSTEM`, `COMMANDS`, and `AUTONOMOUS` capabilities.
The TUI displays the full list and derives one risk level from the most
impactful capability:

- `SAFE`: no declared capability beyond app-owned configuration, cache, and UI state.
- `LOW`: `NETWORK`, `ACCOUNT`, or `FILE_READ`.
- `HIGH`: `FILE_WRITE` or `DELETE`.
- `DANGER`: `SYSTEM`, `COMMANDS`, or `AUTONOMOUS`.

Installation trust is separate. Package-manager installs use generated catalog
plans and postflight executable checks. Script installers always show the
command and require explicit approval, regardless of the app risk level.

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

Main screens use `1` through `8`. Press `?` to open the persistent Help tab,
which explains controls, capabilities, and derived risk levels.
Important actions are intentionally explicit:

- Catalog: `Enter` runs, `I` installs, and `U` uninstalls the current app.
  App details preview app-specific key hints, T4E controls, and a plain-language
  explanation next to the capability-derived risk level.
- Install queue: `x` runs one item, `X` runs queued items sequentially.
- Activity: `Up`/`Down` or `j`/`k` scroll one row, `PageUp`/`PageDown`
  scroll ten rows, `Home`/`End` jump to the newest/oldest entry, and `c`
  clears the log. New entries include local time and UTC offset.
- Workspaces: `Enter` starts and opens, `a` reopens, `x` stops all apps, `h`
  hashes a live snapshot, and `I` queues missing tools. The main flow is pack ->
  app -> run. In App View, `Alt+Left`/`Alt+Right` switches apps, `Alt+Backspace`
  returns to the previous screen while keeping apps running, and `Alt+Q` closes
  the current app. `Backspace` and `Esc` are forwarded to the running app.
  Text selection is the default mouse mode; `Alt+M` toggles T4E mouse controls
  for lists, app tabs, scrolling, and App View footer actions.
  `Alt+O` opens an HTTP(S) link from the current app, and `Alt+C` copies the
  original unwrapped link without App View borders. A single link is handled
  immediately; with several links, T4E shows a picker with the newest selected.
  `Ctrl+C` is forwarded to the app; if it terminates, T4E removes the tmux
  window and shows another running app or returns to the previous screen.
- App rows show install readiness and live install state. The detail panel keeps
  the current attempt, channel, recent package-manager output, and failure
  summary visible without opening the install utility.
- Pack screens list interactive apps only. One-shot commands such as `ffmpeg`,
  `jq`, and `ripgrep` remain available to pack installation, global search, and
  the AI control plane as support tools, but are not launched without inputs.
- Apps with registered flags open a launch-options dialog. Space enables an
  option, Left/Right chooses an allowlisted value, and Enter launches it. T4E
  remembers enabled options and selected values per app for the next run.
- Apps with a required positional value, such as tplay, open an input dialog.
- Settings includes `Reset saved preferences`, which restores runtime policy
  defaults and clears remembered app launch options.
  The value is shell-quoted before launch rather than interpreted as a command.
- Package-manager and T4E-managed installations can be removed with `U`;
  removal requires confirmation and verifies that the executable is gone.
  All other keys go to the current app; users do not need tmux commands.
- AI Home: `Enter` composes a request, `x` interrupts, and `A` reviews a
  proposed side effect.

Script installers, DANGER apps, and AI-proposed side effects require an exact typed
confirmation. Codex runs in a read-only sandbox with app-server approvals set
to `never`; T4E remains authoritative for installation and process lifecycle.

On Linux, `x` or `X` handles the package-manager command without requiring the
user to type it. For apt, dnf, pacman, Snap, missing `pipx`, and declared Cargo
system dependencies, T4E temporarily leaves the alternate screen and runs
interactive `sudo -v`; after authentication it returns to the TUI and executes
the approved install noninteractively. Install processes are serialized, apt
waits for an existing dpkg lock, and Cargo apps receive one 30-minute build
attempt by default. Apps can declare a longer verified budget; Termusic uses
60 minutes and requires both `termusic` and `termusic-server` to pass postflight
checks. Cancelling the sudo prompt leaves the queue item unexecuted.

From the catalog, `R` resets a broken or partial installation and immediately
requeues the current verified install plan. This works even when the primary
binary exists but a required companion binary is missing.

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

If the account's configured model requires a newer Codex CLI, T4E retries once
with `gpt-5.4`. Set `T4E_CODEX_FALLBACK_MODEL` to select another compatible
model available through the signed-in plan. T4E never runs `codex update`
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
- App View input is limited to pane IDs discovered inside T4E-managed sessions;
  commands are passed as structured tmux arguments without shell evaluation.
