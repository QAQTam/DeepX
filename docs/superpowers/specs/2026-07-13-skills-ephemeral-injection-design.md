# Skills Management — Ephemeral Injection + Numbered Catalog

> **Goal:** Skills activation returns `OK` as tool result; skill body is injected as an ephemeral system message in the current turn only (not persisted to system_messages). Catalog becomes a numbered, stable system message. The first system message (`[IDENTITY]`) is never touched.

## Current State (Problem)

```
system_messages (persisted, every API call):
├─ [IDENTITY] system prompt           ← stable
├─ [catalog] skill list text          ← inserted by build_context() every call
├─ [DEEPX_SKILL_V1] skill body A      ← upsert_skill_system, persistent
└─ [DEEPX_SKILL_V1] skill body B      ← grows unbounded, triggers cache miss

tool_result for skills(activate) = FULL skill body (3-20KB)
```

Problems:
1. Every `skills(activate)` pollutes `system_messages` → KV-cache invalidated
2. Skill bodies pile up forever → context bloat
3. Catalog is re-inserted on every `build_context()` instead of once at session init
4. Tool result returning full body is redundant (LLM already sees it injected in system messages)

## Target State

### Message structure sent to API

```
┌─────────────────────────────────────────────┐
│ system: [IDENTITY]...          ← 首条，完全固定   │
│ system: [catalog]              ← 编号列表，session init 时写一次 │
│   S1: unsafe-checker — CRITICAL: Use for...  │
│   S2: m15-anti-pattern — Use when reviewing...  │
│   S3: find-docs — Retrieves...              │
│   ...                                       │
├─────────────────────────────────────────────┤
│ user: "用 S1 审查这段代码"                     │
│ assistant: (tool_use: skills(activate, "unsafe-checker")) │
│ tool: OK                                    │
│ system: [DEEPX_SKILL_V1] name: unsafe-checker... │  ← 仅本 turn 可见
│   --- instructions ---                      │
│   # Unsafe Rust Checker...                  │
│ assistant: "检查完毕，发现3处问题..."           │
├─────────────────────────────────────────────┤
│ user: "继续"                                  │  ← 下轮 skill 已消失
│ assistant: "我需要 skills 列表..."             │     需要时重新 activate
│   (tool_use: skills(list))                  │
│ tool: {skills: [...]}                      │
│ assistant: "好的，让我激活 S2..."              │
│   (tool_use: skills(activate, "m15-anti-pattern")) │
│ tool: OK                                    │
│ system: [DEEPX_SKILL_V1] name: m15...       │  ← 再次临时注入
│ assistant: "分析完毕..."                     │
└─────────────────────────────────────────────┘
```

### Key rules

1. **`system: [IDENTITY]`** is the first system message. Set once at session init. Never modified.

2. **`system: [catalog]`** is the second system message. Written once at session init (or on explicit reload). Lists all available skills with stable numeric IDs. Never modified during a session unless user triggers "reload skills".

3. **`system: [DEEPX_SKILL_V1]` skill body** is injected ephemerally: appears only in the current turn's API request, immediately after the `skills(activate)` tool result. Not stored in `system_messages`. Not visible in future turns.

4. **Tool result for `skills(activate)`** returns `{"status":"ok","content":"[OK] skill activated"}` — NOT the full body.

5. **Catalog numbering** is stable within a session. IDs are derived from the sorted skill name list at discovery time. Same name → same ID. Adding/removing skills → catalog changes → numbering changes (acceptable: explicit operation).

## Changes

### 1. deepx-skills — Catalog numbering

Add `render_catalog_numbered()` that produces:

```text
Available skills (use $S{N} or skills(activate, name="...") to load):

S1: unsafe-checker — CRITICAL: Use for unsafe Rust code review and FFI...
S2: m15-anti-pattern — Use when reviewing code for anti-patterns...
S3: find-docs — Retrieves up-to-date documentation...
```

The number prefix `S{N}:` is stable for the session (derived from sorted skill name list).

### 2. deepx-tools/skill.rs — Tool result change

```rust
fn handle_skill(ctx: ToolCallCtx) -> ToolResult {
    match load_activation(&ctx.args) {
        Ok(activation) => {
            ctx.set_skill_activation(activation);  // side effect for bridge
            ToolResult::ok(serde_json::json!({
                "status": "ok",
                "skill": activation.metadata.name,
                "content": format!("[OK] skill '{}' activated. Use the skill instructions above.", activation.metadata.name)
            }).to_string())
        }
        Err(error) => ToolResult { ... }
    }
}
```

### 3. deepx-msglp/agent.rs — Remove upsert_skill_system

`activate_skill()` and `activate_explicit_skills()` no longer call `msg.upsert_skill_system()`. Instead, they store the skill body in a new `AgentState` field: `active_skill_bodies: HashMap<String, SkillActivation>`.

The catalog is set once at session init:
```rust
fn inject_catalog(&mut self) {
    let catalog = self.refresh_skill_catalog();
    // Remove any previous catalog message, insert new one
    self.msg.replace_catalog_message(&catalog.rendered);
}
```

### 4. deepx-message/store.rs — Context building

`build_context_for_gate()` modified to:

a) No longer keep `skills` tool results full (remove from `keep_full` list)
b) After each tool result from `skills(activate)`, inject the corresponding system message:

```rust
for tr in &step.tool_results {
    v.push(tr.clone());  // tool result = "OK"
    
    // If this tool result is from skills(activate), inject skill body
    if let Some(skill_body) = active_skill_bodies.get(&tr.tool_call_id) {
        v.push(Message::system(&render_activation(skill_body)));
    }
}
```

### 5. deepx-proto — Agent2Ui extension (optional)

New event `SkillsChanged { added: Vec<String>, removed: Vec<String> }` for frontend notification.

### 6. deepx-tauri frontend — Reload entry

Add `/reload-skills` slash command or InputBar button that sends `Ui2Agent::ReloadSkills`.

## Active skill bodies storage

`AgentState` gets a new field:

```rust
/// Skill activations from the current turn. Keyed by tool_call_id so
/// build_context_for_gate can inject the body after the tool result.
/// Cleared when a new turn starts.
pub active_skill_bodies: HashMap<String, SkillActivation>,
```

This is NOT `system_messages`. It's turn-scoped. Cleared on `push_user()`.

## API Compatibility

OpenAI-compatible APIs support `system` role messages at any position in the message array (not just at the beginning). The pattern:

```json
[
  {"role": "system", "content": "[IDENTITY]..."},
  {"role": "system", "content": "Available skills: S1, S2..."},
  {"role": "user", "content": "use S1"},
  {"role": "assistant", "tool_calls": [{"function": {"name": "skills"}}]},
  {"role": "tool", "tool_call_id": "call_xxx", "content": "OK"},
  {"role": "system", "content": "[DEEPX_SKILL_V1]\nname: unsafe-checker\n..."}
]
```

This is valid per OpenAI's API spec (system role is "a message from the system" and can appear anywhere). Anthropic's API similarly allows system messages interspersed.

## Verification

1. Integration test: mock OpenAI server, send a turn that triggers `skills(activate)`, verify the API request has `system` message between `tool` and next assistant
2. Unit test: verify `build_context_for_gate` injects skill body after matching tool result
3. Unit test: verify next turn does NOT contain the previous turn's skill body
4. Cache stability test: verify two consecutive `build_context()` calls with no skill changes produce identical system message prefix
