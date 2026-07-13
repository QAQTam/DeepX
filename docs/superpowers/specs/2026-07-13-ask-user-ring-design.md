# ask_user Pause/Resume — Ring Architecture Integration

> **Goal:** Wire ask_user tool calls to properly pause the Loop, wait for user input, feed the answer back as a tool result in the same turn, and resume LLM inference — all within the new Ring architecture.

## Background

The old Loop (`deepx-msglp/src/lib.rs` + `turn.rs`) handles ask_user by detecting a `[USER_QUERY]` prefix in tool results and simply `break`ing out of the gate→tools loop. The turn ends with `TurnEnd`+`Done`, and the user's response arrives as a brand new `Ui2Agent::UserInput` on the next turn. This means:

- ask_user question and answer are in **different turns** — context is fragmented
- No actual pause/suspend mechanism like permission has
- The LLM sees a new user message, not a completed tool result

The new Ring architecture (`deepx-msglp/src/new/`) already defines `YieldReason::AskUser`, `Outcome::YieldToUser`, `TurnState`, and `TurnEngine.suspended` — the skeleton exists but is only wired for `PermissionPending`, not `AskUser`.

## Scope

This spec covers making ask_user work correctly in the Ring architecture. It does NOT cover:

- Migrating the old Loop to the Ring (that's a separate migration effort)
- Adding a dedicated `Agent2Ui::UserQuery` event (the existing `[USER_QUERY]` in `ToolResults` is sufficient for now)
- Multi-session support

## Architecture

```
LLM calls ask_user("Which approach?")
  → deepx-tools/ask_user.rs: returns "[USER_QUERY]{question, options, ...}"
  → TurnEngine: detects [USER_QUERY] in completed round
  → returns Outcome::YieldToUser { reason: YieldReason::AskUser }
  → Loop::apply_outcome: YieldToUser → Idle (turn suspended in TurnEngine.suspended)
  → Frontend: handleToolResults detects [USER_QUERY] → AskDialog pops up
  → User selects/clicks "Option A"
  → Frontend: invoke("cmd_ask_response", { seed, answer: "Option A" })
  → Tauri bridge: send_to_agent(seed, Ui2Agent::AskResponse { answer })
  → Loop::dispatch_one: accepts AskResponse during suspended turn
  → TurnEngine::resume(AskUserAnswer): finds ask_user call ID, pushes tool result, clears suspended
  → returns Outcome::ContinueTurn → Gate continues with ask_user result in context
  → Same turn, LLM sees "tool result: Option A" → continues reasoning
```

## Changes

### 1. Protocol (`deepx-proto/src/agent_protocol.rs`)

Add to `Ui2Agent`:

```rust
/// User's answer to an ask_user prompt. Resumes a suspended turn.
#[serde(rename = "ask_response")]
AskResponse {
    answer: String,
},
```

No new `Agent2Ui` variant needed. The existing `ToolResults` carrying `[USER_QUERY]` prefix is sufficient for frontend detection.

### 2. TurnState + YieldReason (`deepx-msglp/src/new/types.rs`)

`TurnState` must carry the `YieldReason` so the dispatcher can distinguish ask_user from permission pauses:

```rust
pub struct TurnState {
    // ... existing fields ...
    pub reason: YieldReason,
}
```

Add `suspended_reason()` to `TurnEngine`:

```rust
impl TurnEngine {
    pub fn suspended_reason(&self) -> Option<YieldReason> {
        self.suspended.as_ref().map(|s| s.reason)
    }
}
```

### 3. TurnEngine resume path (`deepx-msglp/src/new/engine_turn.rs`)

`TurnEngine::resume()` currently only handles `PermissionPending`. Add `AskUserAnswer` variant:

```rust
pub enum ResumeReason {
    /// Permission approvals resolved — feed approved results and continue.
    PermissionResolved { call_ids: Vec<String> },
    /// User answered an ask_user prompt — feed answer as tool result and continue.
    AskUserAnswer { answer: String },
}

pub fn resume(&mut self, ctx: &mut RingContext, tool: &mut ToolEngine, reason: ResumeReason) -> Outcome {
    match reason {
        ResumeReason::PermissionResolved { call_ids } => {
            // existing logic...
        }
        ResumeReason::AskUserAnswer { answer } => {
            let state = match self.suspended.take() {
                Some(s) => s,
                None => return Outcome::Error("No suspended turn to resume".into()),
            };
            // Find the ask_user tool call from last step
            let ask_call_id = match ctx.agent.msg.find_last_step_tool_call("ask_user") {
                Some(id) => id,
                None => return Outcome::Error("Cannot find ask_user tool call in suspended turn".into()),
            };
            ctx.agent.msg.push_tool_result(&ask_call_id, &answer, true);
            ctx.agent.msg.flush_meta(&ctx.agent.config.model, &ctx.agent.config.reasoning_effort);
            Outcome::ContinueTurn {
                turn_id: state.turn_id,
                round_num: state.round_num + 1,
                usage: state.usage,
            }
        }
    }
}
```

### 3. Loop dispatch (`deepx-msglp/src/new/loop_core.rs`)

- Username: "UserInput" (when suspended for AskUser, UserInput means the user typed without using the dialog — treat as answer):

```rust
if let Some(reason) = self.session.turn.suspended_reason() {
    match (&frame, reason) {
        (Ui2Agent::PermissionResponse { .. }, YieldReason::PermissionPending) => {}
        (Ui2Agent::AskResponse { .. }, YieldReason::AskUser) => {}
        (Ui2Agent::UserInput { .. }, YieldReason::AskUser) => {}
        (Ui2Agent::Cancel, _)
        | (Ui2Agent::ResumeSession { .. }, _)
        | (Ui2Agent::NewSession, _)
        | (Ui2Agent::Shutdown, _) => {}
        _ => { /* reject with error as before */ }
    }
}
```

**b) `try_handle_via_engines`** — add AskResponse handling:

```rust
Ui2Agent::AskResponse { answer } => {
    let outcome = self.session.turn.resume(
        &mut ctx, &mut self.session.tool,
        super::engine_turn::ResumeReason::AskUserAnswer { answer: answer.clone() },
    );
    Some(outcome)
}
```

**c) `try_handle_via_engines`** — also handle `UserInput` during suspended turn for AskUser (user typed instead of clicking dialog):

```rust
Ui2Agent::UserInput { text } => {
    if self.session.turn.is_suspended() {
        // User typed response directly instead of using AskDialog
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

### 4. Frontend (`deepx-tauri/src/store/chat.ts`)

**a) `submitAskAnswer`**: Replace `cmd_send_message` with `cmd_ask_response`:

```typescript
async function submitAskAnswer(answer: string) {
    setAskState({ question: "", options: [], allow_custom: true, show: false });
    try {
        await invoke("cmd_ask_response", { seed, text: answer });
    } catch (e) { console.error(e); }
}
```

**b) `handleToolResults`**: No change. The existing `[USER_QUERY]` detection at line 225 works correctly.

**c) Dismiss behavior**: `dismissAsk` stays as-is (just hides dialog). The user can type a follow-up message directly, which gets handled by the UserInput-in-suspended-turn path above.

### 5. Tauri bridge (`deepx-tauri/src-tauri/src/agent_bridge.rs`)

Add new command and expose:

```rust
#[tauri::command]
fn cmd_ask_response(seed: String, text: String) -> Result<(), String> {
    send_to_agent(&seed, Ui2Agent::AskResponse { answer: text })
}
```

Register in `main.rs` alongside other commands.

### 6. ask_user tool fix (`deepx-tools/src/ask_user.rs`)

| Field | Old | New | Reason |
|-------|-----|-----|--------|
| `risk` | `ToolRisk::Write` | `ToolRisk::Read` | ask_user doesn't modify anything |
| `default_timeout` | `Duration::from_secs(10)` | `Duration::from_secs(0)` | User interaction has no timeout; Loop's Cancel handles interruption |

### 7. MessageStore helper (`deepx-message/src/store.rs`)

Add `find_last_step_tool_call` method to locate the ask_user call ID in the latest assistant step:

```rust
/// Find a tool call by name in the most recent assistant step.
/// Returns the call_id if found.
pub fn find_last_step_tool_call(&self, tool_name: &str) -> Option<String> {
    self.turns().last()?.steps.last()?.assistant.content.iter()
        .find_map(|block| match block {
            deepx_types::ContentBlock::ToolUse { name, id, .. } if name == tool_name => Some(id.clone()),
            _ => None,
        })
}
```

## Error Handling

| Scenario | Outcome |
|----------|---------|
| AskResponse arrives with no suspended turn | Emit `Error("No suspended turn")` |
| Suspended turn has no ask_user tool call in last step | Emit `Error("Cannot find ask_user tool call")` |
| User dismisses AskDialog then types in InputBar | Treated as `UserInput` during suspended turn → feeds as answer (same as dialog click) |
| Cancel arrives during suspended ask_user | Cross-engine reset clears suspended state, Cancelled emitted |
| Session switch during ask_user | Session destroyed, new one created — answer lost (acceptable: user switched context) |

## Verification

After implementation:

1. `cargo check` in workspace root — all crates compile
2. `cargo test -p deepx-taurs` — existing tests pass  
3. `cargo test -p deepx-msglp` — new/modified tests pass
4. Manual test: send a prompt that triggers ask_user, verify dialog appears, answer, verify LLM continues in same turn
