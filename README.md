# T4E

T4E // TERMINAL APPLICATION ENVIRONMENT

```text
                                         ++++++-
          ---------==+*+==--::::::     -@@@@@@#-    ::::::-----------=.
        .%@@@@@@@@@@@@@@@@@@@@@@@:    #@@@@@@.     #@@@@@@@@@@@@@@@@@*:
       =@@@@@@@@@@@@@@@@@@@@@@@+    -@@@@@@+      -@@@@@@@@@@@@@@@@@-
      -==+**######*#@@@@@@%#%%-    #@@@@@@       .%@@@@%##########*:
                  -%@@@@@*       :@@@@@@*  %@=   =@@@@#            .
                 -@@@@@@#       *@@@@@@. +@@@   .@@@@@= ...:-==++:.
                :@@@@@@%      .@@@@@@*  %@@@+   *@@@@@@@@@@@@@@%-
               :%%%%@@%      *@@@@@@:  #@@@@   -@@@@@@@@@@@@@@+
              -***##%#     :%@@@@@*   :@@@@*   ==%@@#---=====.
             -+==++**  .  +@%%%%%%#%%%@@@@@%%@#.=%@*
            :=:--==+  :  #%#%%%%%%@@@%%%%%@@@+:+%#=              .
           .=..:::-     -====++++=-::#####::::+*++=+++++*******#*:.
           -   ..-                  :****-  :========++++++++**- .
          -.  .--.                 .+==+=  :-----------======-
         -. :::.   .              .=--=+    ..
        :-:.                     -=---..
        :                       =-.
                               .
```

ONE TERMINAL. EVERY TOOL. AI AT THE CONTROLS.

Explore the interactive demos at
[andy5090.github.io/tui-4-everything](https://andy5090.github.io/tui-4-everything/).

T4E is a curated terminal application manager and AI-controlled terminal
environment. HOME AI uses either an existing signed-in Codex, Claude, or Gemini
CLI subscription or an API-key connection for OpenAI, Anthropic, Gemini,
Zhipu AI, Kimi, or a custom OpenAI-compatible endpoint. T4E never persists API
keys and disables AI when no provider is ready.

## Requirements

- Rust stable toolchain
- Linux or macOS
- tmux 3.x as the current hidden app process backend
- An authenticated CLI or configured API provider for optional HOME AI
- The relevant package manager (`apt`, Snap, Homebrew, Cargo, or pipx) for installs

## Install a release

GitHub Releases provide portable musl Linux archives for `x86_64`, `i686`, and `aarch64`,
plus the existing macOS archive. Download the archive matching your CPU from the
release page, then verify its adjacent `.sha256` file before extracting it. For
the latest supported Linux release, use the one-command installer:

```bash
curl -fsSL https://raw.githubusercontent.com/andy5090/tui-4-everything/main/install.sh | bash
```

It detects `x86_64`, 32-bit x86 (`i386` through `i686`), or `aarch64`, verifies the release SHA-256 before it
installs, and places `t4e` in `$HOME/.local/bin`. To install a specific version,
append `-s -- --version VERSION`. For a manual, independently checkable
installation on x86_64 Linux (replace `VERSION` with the release version):

```bash
curl -LO https://github.com/andy5090/tui-4-everything/releases/download/vVERSION/t4e-VERSION-linux-x86_64-musl.tar.gz
curl -LO https://github.com/andy5090/tui-4-everything/releases/download/vVERSION/t4e-VERSION-linux-x86_64-musl.tar.gz.sha256
sha256sum -c t4e-VERSION-linux-x86_64-musl.tar.gz.sha256
tar -xzf t4e-VERSION-linux-x86_64-musl.tar.gz
mkdir -p "$HOME/.local/bin"
install -m 755 t4e-VERSION-linux-x86_64-musl/t4e "$HOME/.local/bin/t4e"
```

On macOS, use the matching `t4e-VERSION-macos-arm64.tar.gz` archive and verify
with `shasum -a 256 -c <archive>.sha256`. On Linux ARM64, replace the archive
label with `linux-aarch64-musl`; on 32-bit x86 use `linux-i686-musl`. The archive also contains the editable Registry
copies and release documentation.

Ensure `$HOME/.local/bin` is on `PATH` (for example, add
`export PATH="$HOME/.local/bin:$PATH"` to your shell profile), reopen the
shell, and run `t4e`. T4E requires `tmux` 3.x to launch managed applications
and a ready AI provider for HOME AI. Without one, application browsing and
management continue normally while AI input stays disabled. Rust is only
required when building from source.

After the first installation, T4E can check for and install its own verified
releases without rerunning the installer command:

```bash
t4e update --check
t4e update
```

Use `t4e update --version VERSION` to install a specific release. `t4e upgrade`
is an alias for the same command. Self-update selects the release for the current
OS and CPU, verifies the adjacent SHA-256 file, and atomically replaces the
running executable so a failed download or verification leaves the installed
version unchanged. It requires `curl` or `wget` plus `tar`, and the executable's
directory must be writable by the current user.

Rerunning the installer remains available as a recovery path. To uninstall the
installer-managed binary, run
`curl -fsSL https://raw.githubusercontent.com/andy5090/tui-4-everything/main/install.sh | bash -s -- --uninstall`.
Removing `$HOME/.local/bin/t4e` does the same; neither option removes apps
installed through T4E, which can be removed individually from the catalog with
`U` before uninstalling.

## Release process

Push a version tag in the form `vVERSION`. The `Real release gates` workflow
runs Gates 1 through 5 first, then builds the existing macOS ARM64 binary and
cross-compiles the portable `x86_64-unknown-linux-musl`,
`i686-unknown-linux-musl`, and `aarch64-unknown-linux-musl` binaries on Ubuntu
with Zig. The ARM64 and i686 validations run through QEMU rather than assuming
native runners. Only after all
package jobs succeed does the workflow publish the GitHub Release, its archives,
and their SHA-256 files. The Linux asset names are deterministic:
`t4e-VERSION-linux-x86_64-musl.tar.gz` and
`t4e-VERSION-linux-aarch64-musl.tar.gz`, plus
`t4e-VERSION-linux-i686-musl.tar.gz`.

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
| Editors | Termleaf, Micro, Helix, LazyVim |
| AI | Claude Code, Codex CLI, OpenCode |
| System | ASCII Camera, Fastfetch |
| Utilities | Glow, VisiData |
| Games | bastet, ninvaders, nudoku |
| Entertainment | Figlet, cowsay, fortune, cmatrix, Asciiquarium, tty-clock, Big Clock, nyancat, pipes.sh, and visual utilities |

Support commands such as `mpv`, `yt-dlp`, `ffmpeg`, `jq`, `ripgrep`, and
`lolcat` are internal dependencies and remain available through explicit
catalog search, but are not shown as applications on HOME.
LazyVim uses an isolated `t4e-lazyvim` profile, leaving an existing Neovim
configuration untouched. Termleaf provides a distraction-free writing editor
with English and Korean input guidance. YouTube TUI provides browsing and
search; tplay asks for a media URL or local path before rendering the media as
terminal ASCII.
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

Applications can also ship inside the t4e executable as builtin apps. Their
catalog installers use the `builtin` method, they always count as installed,
they never run a package manager, and they cannot be uninstalled. Big Clock is
the first builtin app: an extra-large digital clock that scales its digits to
fill the terminal, launched as a hidden `t4e builtin big-clock` subcommand
inside the managed terminal. It lives in the Entertainment category with
labs exposure.

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

The primary navigation is `HOME`, `Activity`, `Settings`, and `Help`.
HOME contains Quick Access, categorized Apps, and a compact
fastfetch system summary with the detected OS ASCII logo and native fastfetch
colors. A persistent search input sits above Quick Access and can be focused by
clicking it or pressing `Ctrl+F`. Left/Right switches between the app views and app
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
- Settings includes a persistent Theme selector. `Future` retains T4E's cyan
  phosphor core with a Tron-inspired electric-orange signal, while `Amber`,
  `Retro Green`, and `Terracotta` provide complete alternatives. `Reset saved
  preferences` restores runtime policy defaults and clears remembered app
  launch options.
  The value is shell-quoted before launch rather than interpreted as a command.
- Package-manager and T4E-managed installations can be removed with `U`;
  removal requires confirmation and verifies that the executable is gone.
  All other keys go to the current app; users do not need tmux commands.
- AI lives in HOME's assistant rail rather than a separate tab. `a` focuses the
  composer, while Settings selects and configures every subscription or API
  connection in one flow, and `x` interrupts supported turns. A persistent
  `REQUEST → REVIEW → RUN` rail shows whether T4E is interpreting the prompt,
  validating or awaiting approval for an action, or executing it. Providers
  may propose catalog search, install planning, a pinned T4E-verified update, or
  an app launch. Settings offers `Auto` by default, immediate `Ask` confirmation,
  and `Bypass`. Auto starts validated action chains without a separate review
  input while retaining high-risk installer and device prompts. Ask opens
  Yes/No as soon as an action is proposed. Bypass skips approval input for the
  complete validated chain, including installer and sensitive-device gates.
  Required launch values not present in the request still need input.
  Validated pipelines may contain two or more exact catalog IDs; missing stages
  are installed sequentially under the selected permission mode before the
  complete pipeline launches once.
- Settings can configure OpenAI (`OPENAI_API_KEY`), Anthropic
  (`ANTHROPIC_API_KEY`), Gemini (`GEMINI_API_KEY`), Zhipu AI
  (`ZHIPU_API_KEY`), Kimi (`MOONSHOT_API_KEY`), and custom OpenAI-compatible
  endpoints. Codex, Claude, and Gemini can switch between detected subscription
  and native API-key modes in the same setup dialog.
  The display name, base URL, model, and environment-variable name are saved;
  a key entered in the dialog exists only for the current T4E process. For a
  future session, export the named variable before starting T4E. Zhipu Coding
  Plan and Kimi's China endpoint can be selected by editing the provider base
  URL.
- Individual app updates are available only when the current platform installer
  declares an exact version, structured version probe, pinned command,
  verification date, and evidence. A different installed version is shown as
  `UPDATE`; `u` queues the exact verified version. Package-manager `latest`
  channels are intentionally not presented as verified updates.

Script installers and DANGER apps retain an explicit review for manual and Auto
actions, showing the risk, capabilities, and exact command before a single Enter
approval; T4E never asks the user to retype a command or confirmation phrase.
Bypass explicitly suppresses those prompts for AI-requested chains. Codex
app-server approvals are denied, Claude runs with tools disabled,
Gemini uses plan mode, and API providers receive only the bounded intent prompt;
T4E remains authoritative for installation, verified updates, and process
lifecycle.

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

## Branding

The README wordmark is mechanically derived from a high-contrast source image
created with Codex's subscription-included image generation. The source PNG and
plain ASCII output are stored in [`assets/branding`](assets/branding).

Regenerate the 74-column result with:

```bash
scripts/image-to-ascii.sh \
  assets/branding/t4e-ascii-source.png \
  74 22 80 '1500:620:(iw-1500)/2:(ih-620)/2' \
  ' .:-=+*#%@' 1.5 -0.04 \
  > assets/branding/t4e-ascii.txt
```

The converter requires `ffmpeg`, `od`, and `awk`. Width, height, brightness
threshold, an optional FFmpeg crop expression, character ramp, contrast, and
brightness are positional arguments.

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
