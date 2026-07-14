# DeepX Tauri Destructive UI Redesign Plan Suite

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the DeepX desktop visual system with a focused task transcript where completed reasoning and tool activity collapse behind one process disclosure.

**Architecture:** The suite first hardens protocol and raw session state, then builds a pure projection layer, replaces the conversation renderer, replaces the application shell/composer, and finally removes the legacy UI. The four plans are sequential; each ends in a testable integration gate and a commit boundary.

**Tech Stack:** Rust 2024, `deepx-proto`, `deepx-tools`, `deepx-msglp`, Tauri 2, SolidJS 1.9, TypeScript 6, Vitest 4, CSS.

**Approved Spec:** `docs/superpowers/specs/2026-07-14-deepx-tauri-destructive-ui-redesign.md`

## Global Constraints

- Existing unrelated worktree changes are user-owned; stage only files named by the active task.
- Do not run workspace-wide formatting; format only touched Rust files.
- Protocol facts remain complete; disclosure expansion remains frontend-only state.
- Only a backend-provided `high` permission risk uses a red solid approval button.
- Completed turns render only prompt, collapsed process disclosure, and final answer at top level.
- No permanent right rail, top telemetry strip, tool-card wall, avatars, or role labels remain after final cleanup.
- Do not add unsupported Projects, Scheduled Tasks, Plugins, Pull Requests, cloud sync, or collaboration features.
- A temporary `deepx:new-conversation-ui` comparison flag must be deleted before the final integration commit.
- Validate light/dark themes at 1920x1080, 1366x768, and a narrow desktop window.

---

## Execution Order

1. [Foundation plan](2026-07-14-deepx-tauri-foundation-plan.md)
2. [Conversation plan](2026-07-14-deepx-tauri-conversation-plan.md)
3. [Shell and composer plan](2026-07-14-deepx-tauri-shell-plan.md)
4. [Integration and cleanup plan](2026-07-14-deepx-tauri-integration-plan.md)

Each plan must pass its own gate before the next begins. Do not parallelize protocol DTO changes with consumers that depend on the generated TypeScript output.

## Spec Coverage

| Approved spec area | Implementation owner |
|---|---|
| baseline defects and command registration | Foundation Task 1 |
| backend permission risk and protocol DTOs | Foundation Tasks 2-3 |
| exhaustive events, raw state, final-round retention | Foundation Task 4 |
| exec ordering, projection, aggregation | Foundation Task 5 |
| completed/running process disclosure | Conversation Tasks 1-3 |
| permission, ask-user, and plan gates | Conversation Task 4 |
| live/restore transcript parity | Conversation Task 5 |
| task sidebar and thread header | Shell Task 1 |
| environment overlay and code deltas | Shell Task 2 |
| safe follow-up queue and composer | Shell Tasks 3-4 |
| focused App shell and empty workspace | Shell Task 5 |
| Skills, usage, and Settings | Integration Tasks 1-2 |
| native titlebar | Integration Task 3 |
| error/cancel/restore/accessibility fixtures | Integration Task 4 |
| responsive themes and visual matrix | Integration Task 5 |
| feature-flag and legacy removal | Integration Task 6 |

## File Ownership Map

| Unit | Responsibility |
|---|---|
| `crates/deepx-proto/src/agent_protocol.rs` | Wire DTOs, final-round marker, permission risk |
| `crates/deepx-tools/src/permission.rs` | Backend risk classification |
| `crates/deepx-tools/src/authorization.rs` | Immutable permission challenge facts |
| `crates/deepx-msglp/src/new/engine_tool.rs` | Permission event emission |
| `crates/deepx-msglp/src/util.rs` | Restored round projection |
| `crates/deepx-tauri/src/store/sessionEventReducer.ts` | Exhaustive Agent2Ui state transitions |
| `crates/deepx-tauri/src/store/rawSession.ts` | Protocol-fact state types |
| `crates/deepx-tauri/src/presentation/turnProjection.ts` | Raw turn to transcript projection |
| `crates/deepx-tauri/src/presentation/processAggregation.ts` | Deterministic process grouping |
| `crates/deepx-tauri/src/components/process/*` | Process disclosure and details |
| `crates/deepx-tauri/src/components/conversation/*` | Prompt, final answer, turn transcript |
| `crates/deepx-tauri/src/components/shell/*` | Sidebar, header, workspace, environment |
| `crates/deepx-tauri/src/components/composer/*` | Composer and follow-up queue |
| `crates/deepx-tauri/src/styles/tokens.css` | Shared color, spacing, width, and motion tokens |
| `crates/deepx-tauri/src-tauri/tauri.conf.json` | Window decoration and sizing |

## Final Gate

Run from the repository root:

```powershell
pnpm --dir crates/deepx-tauri run build
pnpm --dir crates/deepx-tauri run test:run
cargo check -p deepx-tauri
cargo test -p deepx-tauri
git diff --check
git status --short
```

Expected: all commands exit 0; `git status --short` contains only intentionally changed files for the redesign and pre-existing user-owned changes.
