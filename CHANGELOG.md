# Changelog

All notable changes to T4E are recorded here. The project follows Semantic
Versioning, with changes collected under `Unreleased` until a release version
and date are assigned.

## [Unreleased]

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
- Default-on mouse controls with panel-bounded drag selection, visible
  selection feedback, automatic clipboard copy, border removal, and wrapped
  URL cleanup.

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
- Pass structured ASCII Camera options to mpv using its required option syntax
  and migrate stale managed launchers before execution.
- Preserve fastfetch's native ANSI colors in the HOME Information panel,
  replace saved/install queue summaries with the current running-app count.
- Distinguish HOME panel focus from retained selection: only the focused panel
  shows the selection arrow, with no reversed white row background.
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
