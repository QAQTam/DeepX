# New Loop Architecture: The Ring Model

## Design Goal

Replace the God-Object `Loop` (16 fields, ~28 methods) with a **Ring** architecture:
a central dispatcher routes events to independent **Engine** structs through
a unified `RingContext`. Each Engine owns its domain state and exposes a
single `handle()` method. The Loop becomes a thin orchestrator (~150 lines).

## The Ring Metaphor

```
                     ┌─── RingContext (shared services) ───┐
                     │  agent, emitter, cancel, gate        │
                     │  tool_engine, stats, notify          │
                     │                                      │
  Ui2Agent ──▶ Loop.run() ──▶ dispatch ──▶ Engine.handle()  │
                     ▲                          │           │
                     │         Outcome          │           │
                     └──────────────────────────┘           │
                                                             │
  Each Engine returns an Outcome telling the Loop:          │
    Continue    — keep processing (next engine or idle)      │
    Yield       — wait for user (permission / ask_user)      │
    Restart     — start new turn (tools completed → gate)    │
    Complete    — turn finished, emit Done                   │
    Abort       — cancelled or fatal error                   │
```

The "ring" is the cycle: user_input → Gate → Parse → Admit → Execute → Gate → ...
Each Engine is a station on the ring. The Loop is the track.

## Module Map

```
src/new/
├── mod.rs              Module declarations + re-exports
├── DESIGN.md           This file
├── types.rs            Shared types: Outcome, RingContext, TurnState
├── loop_core.rs        Loop struct (thin dispatcher, ~150L)
├── engine_turn.rs      TurnEngine: gate→tools cycle (~500L)
├── engine_tool.rs      ToolEngine: admit→execute→result (~250L)
├── engine_session.rs   SessionEngine: create/resume/config (~150L)
├── engine_input.rs     InputEngine: user input → turn start (~80L)
├── engine_compact.rs   CompactEngine: context summarization (delegates to compact.rs)
├── engine_misc.rs      Undo, Dashboard, Notifications (~100L)
```

## Module Count Comparison

|                | Old (current) | New (ring) |
|----------------|---------------|------------|
| Core loop      | lib.rs 993L   | loop_core.rs ~150L |
| Turn engine    | turn.rs 661L  | engine_turn.rs ~500L |
| Tool engine    | tool_exec.rs + permission.rs 443L | engine_tool.rs ~250L |
| Session engine | lib.rs (scattered) + lifecycle.rs 130L | engine_session.rs ~150L |
| Input engine   | lib.rs handle_user_input ~90L | engine_input.rs ~80L |
| Compact        | compact.rs 286L | engine_compact.rs ~50L (thin) |
| Misc           | lib.rs (scattered) | engine_misc.rs ~100L |
| Types/shared   | lib.rs structs | types.rs ~60L |
| **Total**      | **~2600L** | **~1400L** |

## Outcome Enum (the Ring Interface)

```rust
/// Every Engine returns an Outcome telling the Loop what to do next.
pub enum Outcome {
    /// Turn is complete — Loop emits TurnEnd + Done, returns to Idle.
    TurnComplete {
        turn_id: String,
        usage: Option<UsageInfo>,
    },

    /// Turn needs another lap around the ring (tools executed, back to gate).
    ContinueTurn {
        turn_id: String,
        round_num: u32,
        last_usage: Option<UsageInfo>,
    },

    /// Turn is suspended — waiting for user (permission dialog or ask_user).
    /// Loop must not process any more UserInput until this is resolved.
    YieldToUser {
        turn_id: String,
        reason: YieldReason,
    },

    /// Session needs to be created before continuing.
    NeedSession,

    /// A command was fully handled, return to idle.
    Handled,

    /// Fatal error — emit Error event, return to idle.
    Error(String),

    /// Loop should shut down.
    Shutdown,
}

pub enum YieldReason {
    PermissionPending {
        call_ids: Vec<String>,
    },
    AskUser,
}
```

## Comparison: Current vs New

### 1. Tool Execution

| Aspect          | Current (old)                       | New (ring)                          |
|-----------------|-------------------------------------|-------------------------------------|
| UI tool call    | Loop::handle_tool_call → tool_exec  | Loop dispatches → ToolEngine::handle(UiToolCall{...}) |
| LLM tool call   | Inline in run_llm_turn (150L block) | ToolEngine::execute_parallel() called from TurnEngine |
| Permission      | Scattered: 3 files, 2 code paths   | ToolEngine::admit() single entry point |
| Progress drain  | Loop::drain_tool_progress           | ToolEngine::drain_progress()        |
| Write conflicts | conflict::resolve_write_conflicts   | Same, called by ToolEngine          |
| Code stats      | Loop::code_stats field              | StatsCollector owned by LoopContext |

### 2. ask_user

| Aspect          | Current                             | New                                 |
|-----------------|-------------------------------------|-------------------------------------|
| Detection       | Inline check in run_llm_turn        | ToolEngine returns ToolOutcome::AskUser |
| Pause mechanism | break the loop, implicit            | TurnEngine returns Outcome::YieldToUser(AskUser) |
| Resume          | Next UserInput restarts turn        | Same — InputEngine detects pending ask_user |

### 3. Cancel

| Aspect          | Current                             | New                                 |
|-----------------|-------------------------------------|-------------------------------------|
| Trigger         | CancelToken + global CANCEL flag    | CancelToken (global flag removed)   |
| Propagation     | Manual check at 5+ points           | RingContext::check_cancel() single point |
| Gate abort      | Arc<AtomicBool> passed to gate       | Same, passed via RingContext        |
| Tool abort      | deepx_tools::CANCEL static           | CancelToken cloned into tool threads |
| Cleanup         | remove_last_step_if_incomplete      | TurnEngine::rollback_last_step()    |

### 4. Undo

| Aspect          | Current                             | New                                 |
|-----------------|-------------------------------------|-------------------------------------|
| Trigger         | Ui2Agent::UndoTurn                  | Same                                |
| Execution       | Loop::handle_undo_turn (33L inline) | MiscEngine::handle_undo()           |
| Side effects    | snapshot_full + SessionRestored     | Same, via RingContext emitter       |

### 5. Recall / Retry

| Aspect          | Current                             | New                                 |
|-----------------|-------------------------------------|-------------------------------------|
| Where           | Gate layer (openai.rs)              | Unchanged — gate handles it         |
| Loop awareness  | StreamEvent::Retrying → Agent2Ui::Error | Same, TurnEngine forwards event   |
| Turn recovery   | had_error flag in run_llm_turn      | Outcome::Error returned by TurnEngine |

### 6. Message Loop

| Aspect          | Current                             | New                                 |
|-----------------|-------------------------------------|-------------------------------------|
| Structure       | 28 methods on Loop struct           | 6 Engines + 1 Dispatcher           |
| Dispatch        | match Ui2Agent in dispatch()        | Route to Engine, match Outcome     |
| Interrupt check | drain_pending / check_interrupts    | Single method on Loop: poll_pending() |
| Pending queue   | 4 bool fields                       | PendingState enum                   |

### 7. Other Features

| Feature         | Current                             | New                                 |
|-----------------|-------------------------------------|-------------------------------------|
| Session create  | lifecycle::create_session           | SessionEngine::create()             |
| Session resume  | Loop::handle_resume_session (64L)   | SessionEngine::resume()             |
| Config reload   | Loop::handle_reload_config (22L)    | SessionEngine::reload_config()      |
| Compact         | compact::handle_compact             | CompactEngine delegates to same     |
| Dashboard       | Loop::emit_dashboard (50L inline)   | MiscEngine::emit_dashboard()        |
| Notifications   | Loop::notify field + inline logic   | NotifyHandle in RingContext         |

## Key Differences Summary

| Dimension       | Old Design                          | New Design                          |
|-----------------|-------------------------------------|-------------------------------------|
| State ownership | Loop owns everything (16 fields)    | Each Engine owns its domain         |
| Method access   | Any method can touch any field       | Engine only accesses its own state  |
| Testability     | Must construct full Loop            | Each Engine testable in isolation   |
| Cancellation    | CancelToken + global static         | CancelToken only (no global)        |
| Permission      | 2 separate code paths (UI/LLM)      | Single ToolEngine::admit() path     |
| Turn lifecycle  | Implicit (break/continue in loop)   | Explicit Outcome enum driven        |
| New features    | Add field + 3 call sites            | New Engine or extend existing       |
| External API    | new_ipc() + run()                   | Same (unchanged)                    |

## Session Architecture

### Single-Session (current)

```text
Loop
├── session: SessionBundle          ← active session
│   ├── agent: AgentState           ← MessageStore, Config, SessionMeta
│   ├── stats: StatsCollector       ← code deltas
│   ├── turn: TurnEngine            ← suspended turn state
│   └── tool: ToolEngine            ← pending approvals, trusted folders
├── session_eng: SessionEngine      ← create / resume / reload
├── input: InputEngine              ← user input → turn start
├── compact: CompactEngine          ← context summarization
└── misc: MiscEngine                ← undo, dashboard, mode, notify
```

Session switch flow:
1. `session.flush()` — persist message store + code stats
2. `session_eng.resume(target_seed)` — load target session into `session.agent`
3. `session.turn.reset()` + `session.tool.reset()` — clear session engine state
4. Emit `SessionRestored` / `SessionCreated`

### Multi-Session (future)

```text
Loop
├── sessions: HashMap<String, SessionBundle>   ← seed → bundle
├── active_seed: String
├── session_eng: SessionEngine
├── input: InputEngine
├── compact: CompactEngine
└── misc: MiscEngine
```

Key changes needed for multi-session:
1. **IPC protocol**: `Ui2Agent` variants need `session_id` field (biggest blocker)
2. **SessionBundle**: already designed as swappable unit — no changes needed
3. **Engine trait**: unchanged — engines operate on `&mut RingContext` which
   borrows from the active `SessionBundle`
4. **LRU eviction**: limit in-memory sessions to prevent memory pressure

```rust
// Future sketch:
fn switch_session(&mut self, seed: &str) {
    // Flush current
    if let Some(bundle) = self.sessions.get(&self.active_seed) {
        // bundle is behind &self — need interior mutability or entry API
    }
    // Evict oldest if over limit
    if self.sessions.len() >= MAX_CACHED_SESSIONS {
        // evict LRU
    }
    // Load or create target
    self.sessions.entry(seed.to_string())
        .or_insert_with(|| SessionBundle::load(seed));
    self.active_seed = seed.to_string();
}
```

### Panic Recovery Design

```text
dispatch_one() →
  ┌─ safe_dispatch(|| { ... }) ─┐
  │  try_handle_via_engines()   │
  │  apply_outcome()            │
  │  (recursive ContinueTurn)   │
  └─────────────────────────────┘
           │
           │ panic?
           ▼
  reset_all_engines()
  ├── session.turn.reset()     ← clear suspended turn
  ├── session.tool.reset()     ← clear pending approvals
  ├── session.stats = new()    ← discard partial stats
  ├── session_eng.reset()      ← no-op
  ├── input.reset()            ← no-op
  ├── compact.reset()          ← no-op
  └── misc.reset()             ← no-op
  cancel.clear()
  emit Error(recovered) + Done
  → continue loop
```

### Undo Consistency (Cross-Engine Transaction)

```text
UndoTurn("t5") →
  1. session.turn.reset()      ← clear if "t5" was suspended
  2. session.tool.reset()      ← clear approvals referencing "t5" tool_call_ids
  3. agent.msg.truncate_before_turn("t5")
  4. agent.msg.snapshot_full()
  5. emit SessionRestored
```
