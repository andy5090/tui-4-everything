# Changelog

All notable changes to T4E are recorded here. The project follows Semantic
Versioning, with changes collected under `Unreleased` until a release version
and date are assigned.

## [Unreleased]

## [0.4.0] - 2026-08-09

### Added

- Add `t4e update`, `t4e update --check`, and version-pinned self-update with
  platform-aware GitHub release selection, SHA-256 verification, and atomic
  executable replacement.
- Let HOME AI launch validated pipelines of two or more catalog applications
  and autonomously resolve a bounded YouTube search into `tplay` playback.
- Add Termux as a catalog platform and provide a Termux-native ASCII Camera
  launcher using Termux:API and Chafa.

### Changed

- Replace the two-step AI proposal review with `Bypass`, `Auto`, and `Ask`
  permission modes. Auto-approved install plans now start immediately, Ask
  opens confirmation as soon as an action is proposed, and Bypass continues
  validated AI action chains through installer and device approval gates.
- Start composing in the HOME assistant as soon as text is typed, retain the
  complete conversation, and support keyboard and mouse-wheel history scrolling.
- Update the T4E-verified Termleaf release from 0.3.0 to 0.3.5.

### Fixed

- Keep the newest assistant message visible when conversation history exceeds
  the panel height.
- Prevent wide-character continuation cells from inserting spaces when Korean
  text is copied with mouse drag selection.
- Install an isolated current `yt-dlp` runtime for `tplay` searches so resolved
  YouTube URLs reach the managed player reliably.

## [0.3.1] - 2026-08-03

### Fixed

- Extend repeated full-width ANSI application backgrounds across the complete
  App View canvas, including rows beyond the captured content.

## [0.3.0] - 2026-08-01

### Added

- Termleaf as a default Editors application, with platform-aware installation
  metadata.
- Per-application update discovery and installation limited to versions verified
  by T4E for the active platform.
- HOME AI support for Zhipu AI, Kimi, and custom OpenAI-compatible Chat
  Completions providers, with editable model/base URL profiles, environment or
  session-only API keys, and secret-safe transport.
- One unified AI connection setup for Codex/OpenAI, Claude/Anthropic, Gemini,
  Zhipu AI, Kimi, and custom endpoints. Codex, Claude, and Gemini support both
  detected subscriptions and native API-key mode.
- Yes/No AI action confirmation with `Ask`, `Safe only`, and `All bounded`
  authorization levels, while retaining separate installer and device gates.
- Portable Linux `x86_64` and `aarch64` musl release archives, plus a
  checksum-verifying installer with version, prefix, upgrade, and uninstall
  support.
- Linux `i686` release artifacts and QEMU validation, alongside macOS ARM64
  build and test coverage in the release workflow.

### Fixed

- Restore `Tab` as the HOME panel switcher and use `Shift+Tab` alone to cycle
  HOME, Activity, Settings, and Help, including from focused HOME inputs.
- Move active AI provider selection into Settings, persist the preference, and
  keep HOME's provider display read-only.
- Resolve AI-proposed app targets against exact catalog IDs or case-insensitive
  catalog names before bounded launch approval.
- Open dashboard Help directly with `F1` without intercepting keys inside a
  running app or an active confirmation dialog.
- Move HOME search to `Ctrl+F`; `/` now opens Assistant input with a slash so it
  can begin a skill or command and remains normal text while composing.

## [0.2.0] - 2026-07-27

### Added

- OS-style HOME Quick Access for running, favorite, and recent applications,
  plus categorized Apps views for all and installed applications.
- Internet, Media, Files, Editors, AI, System, Utilities, Games, and
  Entertainment application categories.
- Compact fastfetch system information and OS ASCII logo on HOME with a
  built-in fallback.
- Privacy-aware ASCII Camera launcher backed by mpv.
- Remembered `MPV`, `TCT`, and `CACA` external video renderer choices for
  Yewtube and YouTube TUI.
- Figlet with prompted message input, font, centering, width, and optional
  managed rainbow output.
- Persistent HOME search input above Quick Access with keyboard and mouse
  focus.
- Large ASCII T4E wordmark in the project README.
- Live selected-app installation output at the bottom of HOME Information.
- Default-on mouse controls with panel-bounded drag selection, visible
  selection feedback, automatic clipboard copy, border removal, and wrapped
  URL cleanup.
- Syntax-like Activity highlighting for timestamps, event types, tool output
  streams, successful operations, and failures.
- Codex-generated T4E source artwork, its 74-column ASCII rendering, and a
  reproducible `ffmpeg`-based image-to-ASCII conversion script.
- Builtin application support: catalog apps can ship inside the t4e
  executable, always count as installed, skip package-manager installs, and
  cannot be uninstalled.
- Big Clock, the first builtin application, renders an extra-large digital
  clock that preserves glyph proportions while scaling to the terminal, with
  optional seconds, 12-hour, UTC, date, and ANSI color launch options plus a
  `-f` fill mode that stretches digits to the pane. While running, `c`/`C` or
  the arrow keys cycle the clock color through eight palette colors, an
  animated rainbow gradient, and a hue-cycling solid color, with smooth
  hue-rotation transitions between solid colors. The black palette color
  renders on a white canvas so the clock never disappears.

### Changed

- Replaced user-facing Packs with application-level Apps and Category
  navigation.
- Removed fixed-purpose workspaces from primary navigation and AI actions.
- Defined Codex app-server as the first adapter in a provider-neutral AI
  roadmap that also covers Claude runtimes, Anthropic API, and
  OpenAI-compatible APIs.
- Report managed sessions as background applications rather than workspaces.
- Route YouTube external playback through a shared T4E-managed mpv launcher
  without changing each application's browsing UI or embedded audio behavior.
- Chain required positional input into launch options so applications can
  safely combine quoted user text with allowlisted flags and output effects.
- Defer launch input and options for missing applications until installation
  succeeds; failed or cancelled installs remain on the application screen.
- Widen the desktop HOME Information panel while retaining minimum app-list
  width and the compact layout at smaller terminal sizes.
- Keep `Alt+Q` as a HOME-level quit command while the persistent search input
  is focused.

### Fixed

- Force migration from the legacy YouTube TUI launcher to a versioned managed
  launcher so reinstall preflight cannot preserve an option-incompatible
  wrapper.
- Preserve the managed current `yt-dlp` path during YouTube TUI playback and
  fall back to normal MPV when TCT or CACA output is unavailable.
- Pass structured ASCII Camera options to mpv using its required option syntax
  and migrate stale managed launchers before execution.
- Preserve fastfetch's native ANSI colors in the HOME Information panel,
  replace saved/install queue summaries with the current running-app count.
- Distinguish HOME panel focus from retained selection: only the focused panel
  shows the selection arrow, with no reversed white row background.
- Show active HOME installs in bold yellow and prioritize `INSTALLING` over a
  stale or pre-existing `INSTALLED` marker.
- Let keyboard navigation move upward from Quick Access into Search and tailor
  HOME key hints to the currently focused view or app list.
- Search across all HOME applications regardless of the previous view, clear
  view selection while typing, and move into results with Down or Right.
- Replace the unused visible mux preference with persistent mouse controls and
  show purpose, scope, limits, and controls for each selected setting.
- Keep install timeouts as an internal safety mechanism with per-app overrides
  instead of exposing the fallback duration as a routine user preference.
- Replace the hand-authored README wordmark with the mechanically converted
  Codex-generated branding asset.
- Upgrade the generated wordmark source to commercial campaign typography and
  preserve its lighting and extrusion with a multi-level ASCII character ramp.
- Spell out horizontal panel switching and vertical row movement in HOME
  keyboard hints instead of using the ambiguous generic `arrows` label.
- Prevent drag selection from combining aligned borders of adjacent panels
  into one synthetic copy region.
- Reclassify lolcat as a hidden output-effect dependency and offer it as a
  remembered `Rainbow output` launch option for cowsay and fortune, with
  automatic dependency installation.

## [0.1.0] - 2026-07-16

### Added

- Initial curated TUI registry, verified installation queue, embedded App View,
  activity history, settings, Codex app-server integration, application
  adapters, CI gates, and release packaging.
