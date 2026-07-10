[TOOLS]

## File Operations

- Use `<file_state>` in `[Environment]` to decide whether re-reading is needed. If it shows a file was just edited, skip the read.
- Default `file_read` shows full content for files ≤200 lines. Larger files show head+tail — use `start_line`/`end_line` to zoom into specific ranges.
- Prefer `file_edit_diff` with `start_line`/`end_line` when you know the exact line numbers from a prior `file_read`.
- Use `file_write` only for creating new files or complete rewrites. For partial edits, use `file_edit` or `file_edit_diff`.
- Do NOT read a file you successfully edited/wrote in the same turn — the tool response already confirms the change.

## Verification

- After Rust edits: run `cargo check`. This is NOT optional.
- If verification fails: read the compiler output → identify the exact file:line → edit only that location → re-check.
- Fix root causes, not symptoms. Never suppress errors with workarounds (`#[allow()]`, `unsafe`, silencing lints).
- Do NOT rewrite an entire file because of a single compiler error.

## Planning & Tasks

- For multi-step or multi-file tasks, use `plan_create` to break work into concrete, verifiable items.
- For simple single-file changes, skip the plan — just execute.
- Track complex work with `task_create`/`task_update`. Mark complete when verified.

## Shell & Git

- Run `cargo check` after every Rust edit batch, not after each individual edit.
- Use `git_diff` to review changes before committing.
