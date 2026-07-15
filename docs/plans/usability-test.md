# t4e v0.1 Usability Test

## Purpose

Validate that a user can complete the primary t4e workflows without source
code knowledge. This is a hands-on usability pass, not another unit-test run.

## Start

Build and start an isolated session:

```bash
cargo build --release --locked
scripts/usability/start_session.sh
```

The session uses its own `XDG_STATE_HOME` under `artifacts/usability`. It does
not record keystrokes or terminal contents. Environment details, persistent
state, and a notes template remain in the timestamped session directory.

## Tasks

1. From Home, inspect a starter pack and return without opening help.
2. Find `ripgrep` in Catalog, favorite it, select it, and add it to the queue.
3. Inspect the queued install, execute it, and locate its result or full log.
   An already-installed result is acceptable.
4. Launch one workspace, return to t4e, attach again, inspect its live hash,
   and stop it. Queue missing tools instead if preflight blocks launch.
5. In AI Home, request a catalog search in natural language and verify that
   the result navigates locally without an approval prompt.
6. Request a workspace launch in natural language, verify the proposed action,
   and confirm that it cannot execute before the exact typed approval.
7. Change one setting, exit, restart the same binary with the session's
   `XDG_STATE_HOME`, and confirm that the setting, favorite, and recent item
   persist.
8. Resize the terminal to a narrow layout and confirm that navigation, focused
   content, prompts, and status messages remain readable.

## Record

For each task, record completion, elapsed time, unexpected key presses, error
messages, and whether recovery was obvious in the generated `notes.md`.

Classify defects as:

- `S0`: data loss, unsafe action, terminal corruption, or uncontrolled command.
- `S1`: a primary task cannot be completed.
- `S2`: recovery is possible but the workflow is misleading or inefficient.
- `S3`: visual, wording, or minor interaction defect.

## Acceptance

- All eight tasks complete without an S0 or S1 defect.
- Install and AI side effects never bypass their confirmation policy.
- Exiting always restores the terminal and saves state.
- No managed tmux session remains after the workspace task.
- S2 and S3 findings are captured with reproduction steps before v0.1 release.
