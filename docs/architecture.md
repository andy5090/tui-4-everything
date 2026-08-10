# Architecture

## Trust Boundary

An authenticated CLI or API provider interprets intent and proposes one bounded
action. The t4e state machine
validates IDs against the Registry, presents risk, capabilities, and the exact
command for side effects, accepts a single Enter approval, and then invokes the
existing install, verified-update, or app runtime. T4E never asks the user to
retype a command or confirmation phrase.
Providers never receive direct shell or tmux authority. Codex app-server
requests for command or patch approval are denied by the client.

## Runtime Components

- `app`: terminal lifecycle, state, rendering, and input handling.
- `builtin`: applications embedded in the t4e executable and launched through
  the hidden `builtin` subcommand; catalog entries use the `builtin` install
  method, count as installed, and never touch a package manager.
- `installer`: task materialization, pre/post checks, execution, retry,
  cancellation, exact verified-version checks, diagnostics, and durable logs.
- `mux`: structured tmux invocation, managed-session discovery, interactive
  attach, live snapshots, and reproducibility hashing.
- `codex`: JSONL app-server client and long-lived event service.
- `ai`: provider readiness, Codex adaptation, tool-disabled one-shot Claude and
  Gemini adapters, and a non-streaming OpenAI-compatible Chat Completions
  adapter for Zhipu AI, Kimi, and custom endpoints.
- `mcp`: MCP 2025-06-18 stdio discovery and planning tools.
- `adapters`: mpv JSON IPC plus process-verified yazi/newsboat controls.

## Protocol Compatibility

The app-server client performs `initialize`, sends `initialized`, and verifies
`account/read` before accepting turns. The integration suite also initializes
the installed Codex binary. The opt-in live test exercises a structured turn
against the user's existing Codex login. No Codex credential is read by t4e.
API provider metadata is persisted in user settings, but keys are read from the
configured environment variable or held only in process memory after dialog
entry. HTTPS is required except for loopback custom endpoints, and credentials
are sent to `curl` through standard input so they do not appear in its argv.

MCP initialization negotiates revision `2025-06-18`, advertises only the tools
capability, and reports tool execution failures inside `CallToolResult`.
