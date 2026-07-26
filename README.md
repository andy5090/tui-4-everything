# T4E

```text
+------------------------------------------------------------------------------+
|  T4E :: TERMINAL APPLICATION ENVIRONMENT                         [ ONLINE ]  |
+------------------------------------------------------------------------------+
|                                                                              |
|   TTTTTTTTTTTTTTTTTTTTTTT     444444444       EEEEEEEEEEEEEEEEEEEEEEE        |
|   T:::::::::::::::::::::T     4::::::::4       E:::::::::::::::::::::E       |
|   T:::::::::::::::::::::T    4:::::::::4       E:::::::::::::::::::::E       |
|   T:::::TT:::::::TT:::::T   4::::44::::4       EE::::::EEEEEEEEE::::E        |
|   TTTTTT  T:::::T  TTTTTT  4::::4 4::::4         E:::::E       EEEEEE        |
|           T:::::T         4::::4  4::::4         E:::::E                     |
|           T:::::T        4::::4   4::::4         E::::::EEEEEEEEEE           |
|           T:::::T       4::::444444::::444       E:::::::::::::::E           |
|           T:::::T       4::::::::::::::::4       E:::::::::::::::E           |
|           T:::::T       4444444444:::::444       E::::::EEEEEEEEEE           |
|           T:::::T                 4::::4         E:::::E                     |
|           T:::::T                 4::::4         E:::::E       EEEEEE        |
|         TT:::::::TT               4::::4       EE::::::EEEEEEEE:::::E        |
|         T:::::::::T             44::::::44     E:::::::::::::::::::::E       |
|         T:::::::::T             4::::::::4     E:::::::::::::::::::::E       |
|         TTTTTTTTTTT             4444444444     EEEEEEEEEEEEEEEEEEEEEEE       |
|                                                                              |
+--[ APPS ]--------[ AI ]--------[ AUTOMATION ]--------[ ONE TERMINAL ]--------+
|                                                                              |
|  $ t4e                                                                       |
|  > discover, install, run, and orchestrate terminal applications             |
|  > provider-neutral AI control plane                                         |
|  > every tool, one terminal                                                  |
|                                                                              |
+--------------------------------------------------------- tui-4-everything ---+
```

T4E is a curated terminal application manager and AI-controlled terminal
environment. The current AI backend uses the signed-in `codex` CLI account
through `codex app-server`; provider-neutral support for Claude runtimes,
Anthropic API, and OpenAI-compatible APIs is on the roadmap.

## Requirements

- Rust stable toolchain
- Linux or macOS
- tmux 3.x as the current hidden app process backend
- Codex CLI for the current AI backend
- The relevant package manager (`apt`, Snap, Homebrew, Cargo, or pipx) for installs

## Applications

HOME presents OS-style app views. `Quick Access` contains `Running`,
`Favorites`, and `Recent`. `Apps` contains `All Apps`, `Installed`, and the
application categories; categories only filter applications and never act as
batch-install units.

| Category | Interactive apps |
| --- | --- |
| Internet | Newsboat, Lynx |
| Media | Spotatui, Spotify Player, Ncspot, Cava, Termusic, Shellcast, Yewtube, YouTube TUI, tplay |
| Files | Yazi, ncdu, broot |
| Editors | Micro, Helix, LazyVim |
| AI | Claude Code, Codex CLI, OpenCode |
| System | ASCII Camera, Fastfetch |
| Utilities | Glow, VisiData |
| Games | bastet, ninvaders, nudoku |
| Entertainment | Figlet, cowsay, fortune, cmatrix, Asciiquarium, tty-clock, nyancat, pipes.sh, and visual utilities |

Support commands such as `mpv`, `yt-dlp`, `ffmpeg`, `jq`, `ripgrep`, and
`lolcat` are internal dependencies and remain available through explicit
catalog search, but are not shown as applications on HOME.
LazyVim uses an isolated `t4e-lazyvim` profile, leaving an existing Neovim
configuration untouched. YouTube TUI provides browsing and search; tplay asks
for a media URL or local path before rendering the media as terminal ASCII.
On Linux, both media paths use T4E-managed current `yt-dlp` environments rather
than the frequently outdated Ubuntu repository build. Yewtube and YouTube TUI
offer a remembered external video player choice: `mpv` opens the normal video
window, while `tct` and `caca` render video as colored terminal characters.
T4E applies that choice only to external video playback through its managed
mpv launcher; browsing and embedded audio remain under the application. The
managed player keeps T4E's current `yt-dlp` on `PATH` and falls back to normal
MPV when the installed mpv build lacks the selected terminal renderer.
ASCII Camera reuses mpv's terminal renderer and V4L2 input on Linux, so it does
not install OpenCV. Camera access is classified as `HIGH` and requires approval
on the first launch of each T4E session.

Catalog exposure is internal release metadata: `starter` participates in the
default curated release checks and `labs` is experimental. Exposure and legacy
pack records remain hidden because they are release and compatibility metadata,
not user-facing installation or permission decisions.

Each app declares any combination of `NETWORK`, `ACCOUNT`, `FILE_READ`,
`FILE_WRITE`, `DELETE`, `SYSTEM`, `COMMANDS`, and `AUTONOMOUS` capabilities.
The TUI displays the full list and derives one risk level from the most
impactful capability:

- `SAFE`: no declared capability beyond app-owned configuration, cache, and UI state.
- `LOW`: `NETWORK`, `ACCOUNT`, or `FILE_READ`.
- `HIGH`: `CAMERA_CAPTURE`, `FILE_WRITE`, or `DELETE`.
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

The primary navigation is `HOME`, `AI`, `Activity`, `Settings`, and `Help`.
HOME contains Quick Access, categorized Apps, and a compact
fastfetch system summary with the detected OS ASCII logo and native fastfetch
colors. A persistent search input sits above Quick Access and can be focused by
clicking it or pressing `/`. Left/Right switches between the app views and app
list; Enter enters a list or runs an app, and `?` opens Help. While the selected
app is installing, the bottom of Information shows its channel, attempt, and
four most recent live output lines.
Important actions are intentionally explicit:

- Catalog: `Enter` runs, `I` installs, and `U` uninstalls the current app.
  App details preview app-specific key hints, T4E controls, and a plain-language
  explanation next to the capability-derived risk level.
- Install queue: `x` runs one item, `X` runs queued items sequentially.
- Activity: `Up`/`Down` or `j`/`k` scroll one row, `PageUp`/`PageDown`
  scroll ten rows, `Home`/`End` jump to the newest/oldest entry, and `c`
  clears the log. New entries include local time and UTC offset.
- The main flow is HOME -> Apps or Category -> app -> run. In App View,
  `Alt+Left`/`Alt+Right` switches apps, `Alt+Backspace`
  returns to the previous screen while keeping apps running, and `Alt+Q` closes
  the current app. On HOME, `Alt+Q` exits T4E even when search is focused.
  `Backspace` and `Esc` are forwarded to the running app.
  T4E mouse controls are enabled by default for lists, app tabs, scrolling, and
  App View footer actions. Dragging inside one panel highlights its text and
  automatically copies it on release without the T4E panel border. Wrapped
  single URLs are copied as one clean URL. `Alt+M` disables or restores T4E
  mouse capture when native terminal selection is needed.
  `Alt+O` opens an HTTP(S) link from the current app, and `Alt+C` copies the
  original unwrapped link without App View borders. A single link is handled
  immediately; with several links, T4E shows a picker with the newest selected.
  `Ctrl+C` is forwarded to the app; if it terminates, T4E removes the tmux
  window and shows another running app or returns to the previous screen.
- App rows show install readiness and live install state. The detail panel keeps
  the current attempt, channel, recent package-manager output, and failure
  summary visible without opening the install utility.
- HOME lists interactive applications only. One-shot commands such as
  `ffmpeg`, `jq`, and `ripgrep` remain available to explicit catalog search,
  dependency installation, and the AI control plane as support tools.
- Apps with registered flags or output effects open a launch-options dialog.
  Space enables an option, Left/Right chooses an allowlisted value, and Enter
  launches it. A missing app completes installation before T4E presents its
  input or options; failed and cancelled installs never advance to launch
  configuration. T4E remembers enabled options and selected values per app for
  the next run. `cowsay`, `fortune`, and Figlet offer `Rainbow output`; T4E
  installs the hidden `lolcat` support command when needed and applies it as a
  managed output filter. Figlet prompts for a message before showing its font,
  alignment, width, and color-effect options.
- Apps with a required positional value, such as tplay, open an input dialog.
- Settings includes `Reset saved preferences`, which restores runtime policy
  defaults and clears remembered app launch options.
  The value is shell-quoted before launch rather than interpreted as a command.
- Package-manager and T4E-managed installations can be removed with `U`;
  removal requires confirmation and verifies that the executable is gone.
  All other keys go to the current app; users do not need tmux commands.
- AI: `Enter` composes a request and `x` interrupts it. AI can navigate HOME to
  catalog and installation plans but cannot launch legacy workspace templates.
  The AI tab remains during migration; the target UI integrates provider-neutral
  search, conversation, and app control directly into HOME.

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
packaged executable works outside the source tree. Legacy pack and workspace
records remain embedded for CLI compatibility and validation. Release archives also ship
editable Registry copies, the README, architecture notes, and a SHA-256 file.

## Versioning

T4E follows Semantic Versioning. User-visible changes are recorded in
[`CHANGELOG.md`](CHANGELOG.md) under `Unreleased`; a release moves those entries
to a versioned section with its release date. Navigation or provider contract
breaks require at least a minor version bump while the project is pre-1.0.
Run `t4e --version` (or `cargo run -- --version`) to inspect the build version.

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

- tmux is the hidden app process backend. App View owns normal launch,
  display, keyboard input, switching, and close controls. Zellij layouts
  compile, but App View lifecycle parity is not implemented yet.
- Native adapters are provided for mpv JSON IPC, yazi, and newsboat. The latter
  two verify the tmux pane process and accept only application-specific keys.
- App View input is limited to pane IDs discovered inside T4E-managed sessions;
  commands are passed as structured tmux arguments without shell evaluation.
