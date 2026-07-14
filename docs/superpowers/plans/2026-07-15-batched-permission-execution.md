# Batched Permission Execution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Record every LLM permission decision first, then execute approved tools as one bounded parallel batch and resume the model exactly once.

**Architecture:** `TurnState` owns original tool order, conflicting-writer IDs, deferred authorized tools, and permission IDs awaiting decisions. `ToolEngine` converts each response into a deferred `AdmittedTool`; `TurnEngine` executes the accumulated batch only after the final decision. The frontend keeps one centered permission surface and derives progress from its per-session queue.

**Tech Stack:** Rust 2024, deepx-msglp Ring engine, deepx-tools, Tauri 2, SolidJS, TypeScript, Vitest.

## Global Constraints

- Permission responses record decisions and never execute LLM tools immediately.
- If any permission is pending, automatic and approved tools wait for all decisions.
- Run at most four non-conflicting tools concurrently; conflicting writers keep model order.
- Resume Gate once per completed batch and preserve stdout/stderr `ExecProgress`.
- Keep one centered permission UI; only high-risk approval is red.
- Do not add approve-all or change risk classification.

---

### Task 1: Four-exec lifecycle regression

**Files:**
- Modify: `crates/deepx-msglp/tests/permission_lifecycle.rs`

**Interfaces:**
- Consumes: `run_case`, `tool_round`, `permission_id`, `send`, `assert_no_round_completion`, `collect_through_done`.
- Produces: `llm_four_pending_execs_defer_execution_until_all_resolved`.

- [ ] **Step 1: Write the failing test**

Create four `exec` calls that write four distinct marker files. Receive and sort all IDs; approve three; assert no marker exists and no round completed. Approve the fourth; collect through `Done`; assert all markers exist and the tool round completed once.

```rust
for id in &ids[..3] {
    send(writer, Ui2Agent::PermissionResponse {
        tool_call_id: id.clone(), approved: true, trust_folder: false,
    });
}
assert!(markers.iter().all(|path| !path.exists()));
assert_no_round_completion(receiver);
send(writer, Ui2Agent::PermissionResponse {
    tool_call_id: ids[3].clone(), approved: true, trust_folder: false,
});
let events = collect_through_done(receiver);
assert!(markers.iter().all(|path| path.exists()));
assert_single_completion(&events, 4);
```

- [ ] **Step 2: Verify RED**

Run: `cargo test -p deepx-msglp --test permission_lifecycle llm_four_pending_execs_defer_execution_until_all_resolved -- --exact --test-threads=1`

Expected: FAIL because the first approved exec creates its marker before the fourth decision.

- [ ] **Step 3: Leave the red test uncommitted until Task 2 turns it green**

---

### Task 2: Deferred authorization batch in the Ring

**Files:**
- Modify: `crates/deepx-msglp/src/new/types.rs`
- Modify: `crates/deepx-msglp/src/new/engine_tool.rs`
- Modify: `crates/deepx-msglp/src/new/engine_turn.rs`
- Modify: `crates/deepx-msglp/src/new/loop_core.rs`
- Test: `crates/deepx-msglp/tests/permission_lifecycle.rs`

**Interfaces:**
- Consumes: `AuthorizedToolCall`, `conflict::resolve_write_conflicts`, `PendingAsk`, existing progress/result handlers.
- Produces: `PermissionDisposition::LlmResolved { call_id, admitted }` and deferred fields on `TurnState`.

- [ ] **Step 1: Extend suspended state**

```rust
pub struct TurnState {
    // existing fields remain
    pub deferred_authorized: Vec<AdmittedTool>,
    pub tool_call_order: Vec<String>,
    pub serial_call_ids: HashSet<String>,
}
```

Import `HashSet`; initialize these fields at every `TurnState` construction site.

- [ ] **Step 2: Make LLM permission handling decision-only**

```rust
pub enum PermissionDisposition {
    Ignored,
    UiHandled,
    LlmResolved { call_id: String, admitted: Option<AdmittedTool> },
}
```

Approved LLM challenges return `Some(AdmittedTool)` without calling `execute_authorized`. Rejected and expired decisions write one failed result and return `None`. UI-originated tools retain immediate `execute_and_emit`.

- [ ] **Step 3: Extract the existing bounded executor**

Move the current parallel worker and serial writer blocks into:

```rust
fn execute_admitted_batch(
    ctx: &mut RingContext,
    tool: &ToolEngine,
    mut admitted: Vec<AdmittedTool>,
    tool_call_order: &[String],
    serial_call_ids: &HashSet<String>,
) -> Result<(), ()>
```

Sort by `tool_call_order`, partition by `serial_call_ids`, retain `MAX_PARALLEL_TOOL_WORKERS = 4`, progress draining, result persistence, skill activation, code deltas, cancellation, and panic-to-failed-result conversion.

- [ ] **Step 4: Suspend before execution**

In `run_lap`, derive `tool_call_order` and `serial_call_ids` from the original pending tools. When any permission is pending, store `admission.authorized` in `TurnState.deferred_authorized` and return `YieldToUser` before invoking the executor. With no pending permission, execute normally, then process ask_user or continue Gate.

- [ ] **Step 5: Execute after the final decision**

Change the signature to:

```rust
pub fn handle_permission_resolved(
    &mut self,
    ctx: &mut RingContext,
    tool: &mut ToolEngine,
    call_id: &str,
    admitted: Option<AdmittedTool>,
) -> Outcome
```

Append an approved tool, remove the matching pending ID, and keep yielding while IDs remain. After the last ID, take the saved state, execute the complete batch once, then enter ask_user or emit the completed tool round and run the next Gate lap. Update `loop_core.rs` to pass `admitted`.

- [ ] **Step 6: Verify GREEN**

```powershell
cargo test -p deepx-msglp --test permission_lifecycle -- --test-threads=1
cargo test -p deepx-msglp --lib
cargo check -p deepx-msglp
```

Expected: all exit 0; the new test proves no marker is created before the fourth decision.

- [ ] **Step 7: Commit**

```powershell
git add crates/deepx-msglp/src/new/types.rs crates/deepx-msglp/src/new/engine_tool.rs crates/deepx-msglp/src/new/engine_turn.rs crates/deepx-msglp/src/new/loop_core.rs crates/deepx-msglp/tests/permission_lifecycle.rs
git commit -m "fix(msglp): defer approved tool batches"
```

---

### Task 3: Queue progress and unified permission UI

**Files:**
- Modify: `crates/deepx-tauri/src/store/permissionQueue.ts`
- Modify: `crates/deepx-tauri/src/store/permissionQueue.test.ts`
- Modify: `crates/deepx-tauri/src/App.tsx`
- Modify: `crates/deepx-tauri/src/components/ChatView.tsx`
- Modify: `crates/deepx-tauri/src/components/ChatView.interactions.test.tsx`
- Modify: `crates/deepx-tauri/src/components/interactions/PermissionPrompt.tsx`
- Modify: `crates/deepx-tauri/src/components/interactions/PermissionPrompt.test.tsx`
- Modify: `crates/deepx-tauri/src/styles/interactions.css`
- Delete: `crates/deepx-tauri/src/components/PermissionDialog.tsx`
- Delete: `crates/deepx-tauri/src/components/PermissionDialog.test.tsx`
- Delete: `crates/deepx-tauri/src/styles/permission-dialog.css`

**Interfaces:**
- Produces: `PermissionQueueProgress { current: number; total: number }` and a `progress` prop for `PermissionPrompt`.

- [ ] **Step 1: Write failing tests**

Queue four requests and require progress `1/4`; resolve one and require `2/4`. Render the prompt and assert `第 2/4 项`. Assert only high risk uses the red class and that `App.tsx` no longer imports the legacy stylesheet.

```ts
expect(queue.progress("seed-a")).toEqual({ current: 1, total: 4 });
queue.resolve("seed-a", "call-1");
expect(queue.progress("seed-a")).toEqual({ current: 2, total: 4 });
expect(host.textContent).toContain("第 2/4 项");
```

- [ ] **Step 2: Verify RED**

Run: `npm run test:run -- src/store/permissionQueue.test.ts src/components/interactions/PermissionPrompt.test.tsx src/components/ChatView.interactions.test.tsx`

Expected: FAIL because queue progress does not exist.

- [ ] **Step 3: Implement stable per-seed progress**

```ts
export interface PermissionQueueProgress {
  current: number;
  total: number;
}
```

Track `{ total, resolved }` per seed. Deduplicated enqueue increments total once; successful FIFO resolution increments resolved. `progress(seed)` returns `{ current: resolved + 1, total }` while items remain. `clearSeed` and `clear` remove counters. Pass progress through `App -> ChatView -> PermissionPrompt`.

- [ ] **Step 4: Remove the legacy dialog and normalize colors**

Delete the three legacy files and remove `permission-dialog.css` from App. Keep the centered modal. Use:

```css
.approval-low,
.approval-medium {
  background: var(--text-primary);
  color: var(--bg-primary);
}
.approval-high {
  background: var(--red);
  color: #fff;
}
```

Rejection stays neutral.

- [ ] **Step 5: Verify frontend**

```powershell
npm run test:run
npm run build
```

Expected: all tests pass and Vite exits 0; existing chunk warnings are non-blocking.

- [ ] **Step 6: Commit**

```powershell
git add -A -- crates/deepx-tauri/src
git commit -m "fix(tauri): unify batched permission prompts"
```

---

### Task 4: Cross-layer verification and push

**Files:**
- Verify only; preserve unrelated untracked files.

- [ ] **Step 1: Run final gates**

```powershell
cargo test -p deepx-msglp --test permission_lifecycle -- --test-threads=1
cargo check -p deepx-msglp
Set-Location crates/deepx-tauri
npm run test:run
npm run build
```

Expected: every command exits 0.

- [ ] **Step 2: Review scope**

Run `git status --short`, `git diff HEAD~2 --stat`, and `git log -3 --oneline`. Confirm only planned edits/deletions are committed.

- [ ] **Step 3: Push**

Run: `git push origin refactor/agent-bridge-split`

Expected: remote branch advances to the final implementation commit.

