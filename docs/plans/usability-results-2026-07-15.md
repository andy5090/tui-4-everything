# t4e v0.1 Usability Results

Date: 2026-07-15

Environment: Linux x86_64, tmux PTY, release profile binary, isolated
`XDG_STATE_HOME`, 120x30 and 60x16 terminal sizes. The walkthrough used the
signed-in ChatGPT plan through local `codex app-server`.

## Task Results

| Task | Runtime evidence | Result |
| --- | --- | --- |
| Inspect starter pack and return | Music Pack opened directly from Home and returned with `q` | Pass |
| Find, favorite, select, and queue ripgrep | Global Catalog reset stale scope, search returned one item, favorite and queue persisted | Pass |
| Execute queued install and inspect result | Preflight found `rg`; queue showed Success and “already installed and ready” | Pass |
| Workspace lifecycle or missing-tool route | Video Desk preflight identified two missing tools and `I` queued exactly those tools | Pass by documented alternate |
| AI Catalog request | Incompatible default model was detected, retried with `gpt-5.4`, and navigated to `/ripgrep` | Pass |
| AI Workspace request and approval | Proposal remained visible; no session existed before exact typed approval; approval routed to preflight | Pass |
| Persistence after restart | `ripgrep` favorite, recent item, success queue, and `zellij` setting restored | Pass |
| Narrow terminal | 60x16 showed all seven tabs, back key, focused content, AI input, and status without overlap | Pass |

No managed workspace session remained after the walkthrough, and each TUI
process restored the terminal on exit.

## Defects Found And Fixed

| Severity | Finding | Resolution |
| --- | --- | --- |
| S1 | Current Codex CLI rejected account default model `gpt-5.6-sol` and AI actions failed | Preserve turn error, restart once, and use configurable compatible fallback model (`gpt-5.4` by default) |
| S2 | Pack and AI search filters leaked into later global Catalog visits | Global Catalog entry clears transient pack and query state |
| S2 | Preflight success claimed a tool had just been installed | Report “already installed and ready” and record a precise log event |
| S2 | `Turn completed` overwrote the Workspace approval state | Preserve explicit approval-required status until review |
| S2 | 60-column header and footer hid navigation targets | Add compact tabs and key-first footer below 80 columns |
| S3 | Platform and agent lists were joined inside a single Ratatui line | Render each item as its own `Line` |
| S3 | Raw rate-limit JSON consumed the AI status panel | Display limit name and used percentage summary |

All S0 through S2 findings from this walkthrough are closed. Independent human
testing remains useful for wording and workflow preference, but it is no longer
blocked by a known primary-task failure.
