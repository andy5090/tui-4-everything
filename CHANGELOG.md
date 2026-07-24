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

### Changed

- Replaced user-facing Packs with application-level Apps and Category
  navigation.
- Removed fixed-purpose workspaces from primary navigation and AI actions.
- Defined Codex app-server as the first adapter in a provider-neutral AI
  roadmap that also covers Claude runtimes, Anthropic API, and
  OpenAI-compatible APIs.
- Report managed sessions as background applications rather than workspaces.

### Fixed

- Pass structured ASCII Camera options to mpv using its required option syntax
  and migrate stale managed launchers before execution.
- Reclassify lolcat as a hidden output-effect dependency and offer it as a
  remembered `Rainbow output` launch option for cowsay and fortune, with
  automatic dependency installation.

## [0.1.0] - 2026-07-16

### Added

- Initial curated TUI registry, verified installation queue, embedded App View,
  activity history, settings, Codex app-server integration, application
  adapters, CI gates, and release packaging.
