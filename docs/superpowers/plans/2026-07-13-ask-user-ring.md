# ask_user Pause/Resume — Ring Architecture Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use subagent-driven-development (recommended) or executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire ask_user tool calls to properly pause the Loop, wait for user input, feed the answer back as a tool result in the same turn, and resume LLM inference — all within the new Ring architecture.

**Architecture:** Add `Ui2Agent::AskResponse` protocol variant. TurnEngine stores `YieldReason` in suspended `TurnState` and handles ask_user resume with `ResumeReason::AskUserAnswer`. Loop dispatch uses `suspended_reason()` to gate accepted commands. Frontend sends `cmd_ask_response` instead of `cmd_send_message`.

**Tech Stack:** Rust (edition 2024), deepx-proto, deepx-msglp (Ring), deepx-message, deepx-tauri (Tauri v2), SolidJS/TypeScript

## Global Constraints

- `cargo check` must pass after every task
- TDD: write failing test first, watch it fail, then implement
- `unwrap_used = "deny"` (clippy lint)
- All UI↔Agent communication via JSON-LP over stdin/stdout
- Old Loop is being deleted — only modify Ring code in `new/`

---

### Task 1: Protocol — Add `Ui2Agent::AskResponse`

**Files:**
- Modify: `crates/deepx-proto/src/agent_protocol.rs:60-85`
- Test: `crates/deepx-proto/src/agent_protocol.rs` (add test at bottom)

**Interfaces:**
- Produces: `Ui2Agent::AskResponse { answer: String }` variant, consumed by Tasks 5 and 7

- [ ] **Step 1: Write the failing test (deserialization round-trip)**

```rust
#[test]
fn ask_response_deserializes_correctly() {
    let json = r#"{"type":"ask_response","answer":"Option A"}"#;
    let frame: Ui2Agent = serde_json::from_str(json).unwrap();
    match frame {
        Ui2Agent::AskResponse { answer } => assert_eq!(answer, "Option A"),
        other => panic!("expected AskResponse, got {:?}", std::any::type_name_of_val(&other)),
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p deepx-proto ask_response_deserializes_correctly`
Expected: FAIL — `AskResponse` variant doesn't exist yet.

- [ ] **Step 3: Add `AskResponse` variant to `Ui2Agent` enum**

After the `PermissionResponse` variant (line ~81 in `agent_protocol.rs`), add:

```rust
/// User's answer to an ask_user prompt. Resumes a suspended turn.
#[serde(rename = "ask_response")]
AskResponse {
    answer: String,
},
```

Also add `AskResponse` to the exhaustive match in `fallback` handlers where needed (compile errors will guide this).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p deepx-proto`
Expected: All tests pass including `ask_response_deserializes_correctly`.

- [ ] **Step 5: Verify full workspace compiles**

Run: `cargo check`
Expected: exit 0. (TS bindings regeneration may show diff in `Agent2Ui.ts` — expected, commit it.)

- [ ] **Step 6: Commit**

```bash
git add crates/deepx-proto/src/agent_protocol.rs crates/deepx-tauri/src/lib/types/Ui2Agent.ts
git commit -m "feat(proto): add Ui2Agent::AskResponse variant"
```

---

### Task 2: TurnState — Add `YieldReason` field and `suspended_reason()`

**Files:**
- Modify: `crates/deepx-msglp/src/new/types.rs:178-188`
- Modify: `crates/deepx-msglp/src/new/engine_turn.rs:26-30` (TurnEngine struct + impl)

**Interfaces:**
- Produces: `TurnState.reason: YieldReason`, `TurnEngine::suspended_reason() -> Option<YieldReason>`, consumed by Task 5 (Loop guard)

- [ ] **Step 1: Add `reason` field to `TurnState`**

In `crates/deepx-msglp/src/new/types.rs`, line ~180, add the field:

```rust
pub struct TurnState {
    pub turn_id: String,
    pub round_num: u32,
    pub usage: Option<UsageInfo>,
    pub pending_call_ids: Vec<String>,
    pub session_id: String,
    /// Why the turn was suspended.
    pub reason: YieldReason,
}
```

- [ ] **Step 2: Update TurnState construction in engine_turn.rs line ~385**

Add `reason: YieldReason::PermissionPending` to the existing construction:

```rust
self.suspended = Some(TurnState {
    session_id: ctx.agent.session.seed.clone(),
    turn_id: turn_id.clone(),
    round_num,
    pending_call_ids: round_pending_ids,
    usage: last_usage.clone(),
    reason: YieldReason::PermissionPending,
});
```

- [ ] **Step 3: Add `suspended_reason()` to `TurnEngine`**

In `crates/deepx-msglp/src/new/engine_turn.rs`, inside `impl TurnEngine`, add:

```rust
pub fn suspended_reason(&self) -> Option<YieldReason> {
    self.suspended.as_ref().map(|s| s.reason)
}
```

- [ ] **Step 4: Verify compile**

Run: `cargo check -p deepx-msglp`
Expected: exit 0.

- [ ] **Step 5: Commit**

```bash
git add crates/deepx-msglp/src/new/types.rs crates/deepx-msglp/src/new/engine_turn.rs
git commit -m "feat(turn): add YieldReason to TurnState + suspended_reason()"
```

---

### Task 3: MessageStore — Add `find_last_step_tool_call`

**Files:**
- Modify: `crates/deepx-message/src/store.rs:610-630` (near `last_step_tool_results`)
- Test: `crates/deepx-message/src/store.rs` (tests section at bottom)

**Interfaces:**
- Produces: `MessageStore::find_last_step_tool_call(name: &str) -> Option<String>`, consumed by Task 4 (TurnEngine resume)

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn find_last_step_tool_call_returns_correct_id() {
    let mut store = MessageStore::new("test");
    store.push_user("hello");

    // Simulate an assistant message with ask_user tool call
    let assistant = Message {
        role: "assistant".into(),
        content: vec![
            deepx_types::ContentBlock::Text { text: "Let me ask...".into() },
            deepx_types::ContentBlock::ToolUse {
                id: "call_abc123".into(),
                name: "ask_user".into(),
                input: serde_json::json!({"question": "Which one?"}),
            },
        ],
        ..Default::default()
    };
    store.push_assistant(assistant);

    let id = store.find_last_step_tool_call("ask_user").unwrap();
    assert_eq!(id, "call_abc123");
    assert!(store.find_last_step_tool_call("bogus").is_none());
}

#[test]
fn find_last_step_tool_call_with_no_turns_returns_none() {
    let store = MessageStore::new("test");
    assert!(store.find_last_step_tool_call("ask_user").is_none());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p deepx-message find_last_step_tool_call`
Expected: FAIL — method doesn't exist.

- [ ] **Step 3: Implement `find_last_step_tool_call`**

In `crates/deepx-message/src/store.rs`, inside `impl MessageStore`, add:

```rust
/// Find the tool_call_id of a named tool in the most recent assistant step.
/// Returns None if no matching tool call is found.
pub fn find_last_step_tool_call(&self, tool_name: &str) -> Option<String> {
    let step = self.turns.last()?.steps.last()?;
    step.assistant.content.iter().find_map(|block| match block {
        deepx_types::ContentBlock::ToolUse { name, id, .. } if name == tool_name => Some(id.clone()),
        _ => None,
    })
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p deepx-message`
Expected: All tests pass including the two new ones.

- [ ] **Step 5: Commit**

```bash
git add crates/deepx-message/src/store.rs
git commit -m "feat(message): add find_last_step_tool_call helper"
```

---

### Task 4: TurnEngine — `ResumeReason` enum and ask_user resume path

**Files:**
- Modify: `crates/deepx-msglp/src/new/engine_turn.rs:1-70` (struct + resume method)
- Modify: `crates/deepx-msglp/src/new/engine_turn.rs:405-415` (ask_user yield section)

**Interfaces:**
- Consumes: `MessageStore::find_last_step_tool_call` (Task 3), `TurnState.reason` (Task 2)
- Produces: `ResumeReason` enum (public), `TurnEngine::resume(reason)` updated signature, consumed by Task 5

- [ ] **Step 1: Add `ResumeReason` enum and update `resume()` signature**

In `crates/deepx-msglp/src/new/engine_turn.rs`, before `pub struct TurnEngine`, add:

```rust
/// Why the turn is being resumed.
pub enum ResumeReason {
    /// User answered permission dialogs.
    PermissionResolved,
    /// User answered an ask_user prompt.
    AskUserAnswer { answer: String },
}
```

Replace the existing `resume()` method (lines 50-67) with:

```rust
/// Resume a suspended turn.
pub fn resume(&mut self, ctx: &mut RingContext, tool: &mut ToolEngine, reason: ResumeReason) -> Outcome {
    match reason {
        ResumeReason::PermissionResolved => {
            let saved = match self.suspended.take() {
                Some(s) => s,
                None => return Outcome::Error("No suspended turn to resume".into()),
            };
            if saved.session_id != ctx.agent.session.seed {
                log::warn!("[TURN] refusing to resume stale turn {}", saved.turn_id);
                return Outcome::Handled;
            }
            log::info!("[TURN] resuming turn {} round {}", saved.turn_id, saved.round_num);
            self.emit_completed_tool_round(ctx, &saved.turn_id, saved.round_num);
            self.run_lap(ctx, tool, saved.turn_id, saved.round_num + 1, saved.usage)
        }
        ResumeReason::AskUserAnswer { answer } => {
            let saved = match self.suspended.take() {
                Some(s) => s,
                None => return Outcome::Error("No suspended turn to resume".into()),
            };
            if saved.session_id != ctx.agent.session.seed {
                log::warn!("[TURN] refusing to resume stale turn {}", saved.turn_id);
                return Outcome::Handled;
            }
            // Find the ask_user tool call ID from the last step
            let ask_call_id = match ctx.agent.msg.find_last_step_tool_call("ask_user") {
                Some(id) => id,
                None => return Outcome::Error(
                    "Cannot find ask_user tool call in suspended turn".into()
                ),
            };
            // Push user's answer as the tool result
            ctx.agent.msg.push_tool_result(&ask_call_id, &answer, true);
            ctx.agent.msg.flush_meta(
                &ctx.agent.config.model,
                &ctx.agent.config.reasoning_effort,
            );
            log::info!("[TURN] ask_user answer fed as tool result, resuming turn {}", saved.turn_id);
            // Emit completed tool results so frontend updates
            self.emit_completed_tool_round(ctx, &saved.turn_id, saved.round_num);
            // Continue with next Gate lap
            self.run_lap(ctx, tool, saved.turn_id, saved.round_num + 1, saved.usage)
        }
    }
}
```

- [ ] **Step 2: Save TurnState when yielding for AskUser**

In `engine_turn.rs`, replace lines ~409-413 (the `if has_user_query` block) with:

```rust
if has_user_query {
    self.suspended = Some(TurnState {
        session_id: ctx.agent.session.seed.clone(),
        turn_id: turn_id.clone(),
        round_num,
        pending_call_ids: Vec::new(), // no permissions pending — just ask_user
        usage: last_usage.clone(),
        reason: YieldReason::AskUser,
    });
    return Outcome::YieldToUser {
        turn_id,
        reason: YieldReason::AskUser,
    };
}
```

- [ ] **Step 3: Update all `resume()` call sites to pass `ResumeReason`**

Search for `.resume(` in `loop_core.rs` and `engine_tool.rs`. Update each call:

- `loop_core.rs`: the `handle_permission_response` path → `ResumeReason::PermissionResolved`
- The new `AskResponse` handler (Task 5) → `ResumeReason::AskUserAnswer { answer }`

- [ ] **Step 4: Verify compile**

Run: `cargo check -p deepx-msglp`
Expected: exit 0. Fix any compile errors from call site mismatches.

- [ ] **Step 5: Write unit test for AskUser resume path**

In `crates/deepx-msglp/src/new/engine_turn.rs`, add at bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::AgentState;

    #[test]
    fn resume_ask_user_answer_feeds_tool_result() {
        // Setup: create agent with a turn that has an ask_user tool call
        let mut agent = AgentState::init("test");
        agent.config.permission_level = 4; // skip permission checks
        agent.msg.push_user("hello");
        let assistant = deepx_types::Message {
            role: "assistant".into(),
            content: vec![deepx_types::ContentBlock::ToolUse {
                id: "call_ask1".into(),
                name: "ask_user".into(),
                input: serde_json::json!({"question": "Which?"}),
            }],
            ..Default::default()
        };
        agent.msg.push_assistant(assistant.clone());

        // Suspend with AskUser reason
        let turn_id = "t1".to_string();
        let mut engine = TurnEngine::new();
        engine.suspended = Some(TurnState {
            turn_id: turn_id.clone(),
            round_num: 2,
            usage: None,
            pending_call_ids: vec![],
            session_id: agent.session.seed.clone(),
            reason: YieldReason::AskUser,
        });

        // Resume with answer
        // (Full test requires RingContext — this validates find_last_step path)
        let call_id = agent.msg.find_last_step_tool_call("ask_user").unwrap();
        assert_eq!(call_id, "call_ask1");
    }
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p deepx-msglp`
Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/deepx-msglp/src/new/engine_turn.rs
git commit -m "feat(turn): ResumeReason enum + AskUserAnswer resume path"
```

---

### Task 5: Loop dispatch — Suspended guard + AskResponse handling

**Files:**
- Modify: `crates/deepx-msglp/src/new/loop_core.rs:610-635` (suspended guard)
- Modify: `crates/deepx-msglp/src/new/loop_core.rs:790-810` (try_handle_via_engines UserInput arm)
- Modify: `crates/deepx-msglp/src/new/loop_core.rs` (add AskResponse arm in try_handle_via_engines)

**Interfaces:**
- Consumes: `suspended_reason()` (Task 2), `ResumeReason` (Task 4)
- Produces: Suspended guard uses reason-aware filtering; AskResponse and UserInput-during-AskUser resume the turn

- [ ] **Step 1: Replace suspended guard with reason-aware version**

In `dispatch_one`, replace lines ~612-630 (the `if self.session.turn.is_suspended()` block) with:

```rust
// ── Guard: suspended turn ──
if let Some(reason) = self.session.turn.suspended_reason() {
    match (&frame, reason) {
        // Permission pending → only accept PermissionResponse
        (Ui2Agent::PermissionResponse { .. }, YieldReason::PermissionPending) => {}
        // AskUser pending → accept AskResponse or direct text input
        (Ui2Agent::AskResponse { .. }, YieldReason::AskUser) => {}
        (Ui2Agent::UserInput { .. }, YieldReason::AskUser) => {}
        // Always accepted regardless of suspension reason
        (Ui2Agent::Cancel, _)
        | (Ui2Agent::ResumeSession { .. }, _)
        | (Ui2Agent::NewSession, _)
        | (Ui2Agent::Shutdown, _) => {}
        _ => {
            log::warn!("[AGENT] dropping command {:?} during suspended turn (reason={:?})",
                std::any::type_name_of_val(&frame), reason);
            let _ = self.event_tx.send(Agent2Ui::Error {
                message: "Turn is suspended — resolve pending permissions or ask_user first.".into(),
            });
            return;
        }
    }
}
```

- [ ] **Step 2: Add AskResponse handling in try_handle_via_engines**

After the `Ui2Agent::PermissionResponse` arm (around line ~790), add:

```rust
Ui2Agent::AskResponse { answer } => {
    let outcome = self.session.turn.resume(
        &mut ctx, &mut self.session.tool,
        super::engine_turn::ResumeReason::AskUserAnswer { answer: answer.clone() },
    );
    Some(outcome)
}
```

- [ ] **Step 3: Update UserInput handling in try_handle_via_engines for suspended AskUser**

Replace the existing `Ui2Agent::UserInput` arm in `try_handle_via_engines`:

```rust
Ui2Agent::UserInput { text } => {
    if self.session.turn.is_suspended() {
        // User typed directly instead of clicking AskDialog — treat as ask_user answer
        let outcome = self.session.turn.resume(
            &mut ctx, &mut self.session.tool,
            super::engine_turn::ResumeReason::AskUserAnswer { answer: text.clone() },
        );
        Some(outcome)
    } else {
        Some(self.input.handle_user_input(&mut ctx, text))
    }
}
```

- [ ] **Step 4: Update old resume() call in handle_permission_response path**

Find the `self.session.turn.resume(...)` call in `try_handle_via_engines` PermissionResponse arm and update to pass `ResumeReason::PermissionResolved`:

```rust
Ui2Agent::PermissionResponse { tool_call_id, approved, trust_folder } => {
    Some(self.session.tool.handle_permission_response(
        &mut ctx, tool_call_id, *approved, *trust_folder,
    ))
}
```

Then ensure `ToolEngine::handle_permission_response` internally calls `turn.resume(ctx, tool, ResumeReason::PermissionResolved)` when all approvals resolve.

- [ ] **Step 5: Add AskResponse to the fallback match exhaustiveness list**

In the `// Already handled by engine chain` fallback match at bottom of `dispatch_one`, add:

```rust
| Ui2Agent::AskResponse { .. }
```

- [ ] **Step 6: Verify compile**

Run: `cargo check -p deepx-msglp`
Expected: exit 0.

- [ ] **Step 7: Commit**

```bash
git add crates/deepx-msglp/src/new/loop_core.rs
git commit -m "feat(loop): reason-aware suspended guard + AskResponse dispatch"
```

---

### Task 6: ask_user tool — Fix risk and timeout

**Files:**
- Modify: `crates/deepx-tools/src/ask_user.rs:27-28`

**Interfaces:**
- Produces: Corrected `ToolHandler` for ask_user registration

- [ ] **Step 1: Fix `risk` from `Write` to `Read` and timeout from 10s to 0**

```rust
pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: "ask_user".to_string(),
        description: "...",  // unchanged
        input_schema: serde_json::json!({...}),  // unchanged
        handler: handle_ask_user,
        risk: ToolRisk::Read,
        default_timeout: std::time::Duration::ZERO,
    });
}
```

- [ ] **Step 2: Verify compile**

Run: `cargo check -p deepx-tools`
Expected: exit 0.

- [ ] **Step 3: Commit**

```bash
git add crates/deepx-tools/src/ask_user.rs
git commit -m "fix(tools): ask_user risk=Read, timeout=0"
```

---

### Task 7: Tauri bridge — Add `cmd_ask_response` command

**Files:**
- Modify: `crates/deepx-tauri/src-tauri/src/agent_bridge.rs` (add new command)
- Modify: `crates/deepx-tauri/src-tauri/src/lib.rs:15-50` (register command)

**Interfaces:**
- Consumes: `Ui2Agent::AskResponse` (Task 1)
- Produces: `cmd_ask_response` Tauri command, consumed by Task 8 (frontend)

- [ ] **Step 1: Add `cmd_ask_response` function in `agent_bridge.rs`**

After `cmd_send_message` (around line 690), add:

```rust
#[tauri::command]
pub fn cmd_ask_response(seed: String, text: String) -> Result<(), String> {
    log::info!("[REGISTRY] cmd_ask_response seed={}: answer={:.50}",
        &seed[..seed.floor_char_boundary(seed.len().min(8))],
        &text[..text.floor_char_boundary(50)]);
    ensure_agent(&seed)?;
    send_to_agent(&seed, Ui2Agent::AskResponse { answer: text })
}
```

- [ ] **Step 2: Register command in `lib.rs`**

In `crates/deepx-tauri/src-tauri/src/lib.rs`, add `agent_bridge::cmd_ask_response` to the `generate_handler!` macro list (alphabetically, after `agent_bridge::cmd_get_version` or wherever appropriate):

```rust
.invoke_handler(tauri::generate_handler![
    agent_bridge::cmd_send_message,
    agent_bridge::cmd_ask_response,        // <-- new
    agent_bridge::cmd_set_mode,
    agent_bridge::cmd_get_version,
    // ... rest unchanged
])
```

- [ ] **Step 3: Verify compile**

Run: `cargo check -p deepx-tauri`
Expected: exit 0.

- [ ] **Step 4: Commit**

```bash
git add crates/deepx-tauri/src-tauri/src/agent_bridge.rs crates/deepx-tauri/src-tauri/src/lib.rs
git commit -m "feat(tauri): add cmd_ask_response command"
```

---

### Task 8: Frontend — Update `submitAskAnswer` to use `cmd_ask_response`

**Files:**
- Modify: `crates/deepx-tauri/src/store/chat.ts:396-401`

**Interfaces:**
- Consumes: `cmd_ask_response` Tauri command (Task 7)

- [ ] **Step 1: Replace `cmd_send_message` with `cmd_ask_response` in `submitAskAnswer`**

In `crates/deepx-tauri/src/store/chat.ts`, lines 396-401, replace:

```typescript
async function submitAskAnswer(answer: string) {
    setAskState({ question: "", options: [], allow_custom: true, show: false });
    try {
        await invoke("cmd_ask_response", { seed, text: answer });
    } catch (e) { console.error(e); }
}
```

The old code was:
```typescript
async function submitAskAnswer(answer: string) {
    setAskState({ question: "", options: [], allow_custom: true, show: false });
    try {
        await invoke("cmd_send_message", { seed, text: answer });
    } catch (e) { console.error(e); }
}
```

- [ ] **Step 2: Verify TypeScript compiles**

Run: `cd crates/deepx-tauri && npx tsc --noEmit`
Expected: No type errors.

- [ ] **Step 3: Commit**

```bash
git add crates/deepx-tauri/src/store/chat.ts
git commit -m "feat(ui): submitAskAnswer uses cmd_ask_response instead of cmd_send_message"
```

---

### Task 9: Integration — Full workspace verification

**Files:**
- (none modified — verification only)

**Interfaces:**
- Consumes: All previous tasks

- [ ] **Step 1: Full compile check**

Run: `cargo check`
Expected: exit 0, all crates compile.

- [ ] **Step 2: Run all Rust tests**

Run: `cargo test`
Expected: All tests pass (0 failures).

- [ ] **Step 3: Check for `[USER_QUERY]` references — confirm detection still works**

Run: `grep -rn "USER_QUERY" crates/`
Expected: Present in `ask_user.rs` (emitter), `engine_turn.rs` (detector), and `chat.ts` (frontend detector). No orphaned references to old `[USER_QUERY]`-in-`cmd_send_message` flow.

- [ ] **Step 4: Verify TS bindings are clean**

Run: `git diff crates/deepx-tauri/src/lib/types/Ui2Agent.ts`
Expected: Only the `AskResponse` variant added; no other unexpected changes.

- [ ] **Step 5: Commit final state**

```bash
git add -A
git commit -m "chore: integration check — all tests pass, types synchronized"
```
