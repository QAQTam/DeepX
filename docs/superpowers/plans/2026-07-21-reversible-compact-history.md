# Reversible compact history implementation plan

> **Execution:** this plan is being implemented inline in the current task, with each item verified before the next begins.

**Goal:** retain every raw chat message through compact and repeated compact operations, while restoring only the latest compact context to the agent loop.

**Architecture:** `messages.jsonl` and Turso's `messages` table remain the immutable, dual-written archive. A separate compact-context checkpoint records the active synthetic summary plus retained tail. Resume loads that checkpoint for the LLM; history APIs continue to read the archive. Checkpoints form a parent-linked linear chain.

**Storage contract:**

- `messages.jsonl` / `messages`: append-only raw history after the first compact.
- `compact-context.json` / `session_compact_context`: current active context, checkpoint id, parent id, and archive message count.
- A compact write updates the archive metadata and active checkpoint durably; a failed mirror leaves a recoverable file checkpoint.
- Existing sessions without a checkpoint retain the legacy behavior and load their full history.

## TODO

- [x] Define and persist the compact-context checkpoint in both file and Turso stores.
- [x] Make compact write a checkpoint instead of rewriting raw message history.
- [x] Restore the checkpoint for the agent loop while retaining archive recovery APIs.
- [x] Keep later message appends synchronized into the active checkpoint.
- [x] Add tests for one compact, repeated compact, restart/resume, and JSONL/DB recovery.
- [ ] Run targeted Rust tests and workspace checks; commit the verified changes.

## Verification criteria

1. Raw messages before compact still exist byte-for-byte in JSONL and the database after compact.
2. The next agent loop and a restarted session see the compact marker plus retained tail, not the full raw archive.
3. A second compact links to the first checkpoint and never removes the archive.
4. File and DB checkpoints agree, or the normal mirror recovery process reports/repairs the stale side.
