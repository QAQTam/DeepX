# ask_user Lifecycle and Protocol Design

> **Status:** Approved for implementation on 2026-07-14.
>
> **Supersedes:** `docs/superpowers/specs/2026-07-13-ask-user-ring-design.md`.

## Goal

Make `ask_user` a reliable, identity-safe pause/resume operation in the production new Ring loop. A model may issue one ask containing one or many questions, or multiple independent `ask_user` calls in the same assistant round. The UI must collect the intended answers, the agent must attach exactly one answer result to each original tool call, and the model must not resume until every ask in that round is resolved.

## Scope

This change is intentionally limited to:

- `deepx-tools`: ask argument normalization and schema
- `deepx-proto`: explicit ask request, response, acknowledgement, and error DTOs
- `deepx-msglp/src/new`: suspended-turn state, sequential ask queue, permission handoff, and turn completion
- `deepx-tauri`: IPC commands, per-session ask queue, dialogs/forms, and frontend tests
- `deepx-message`: only if a small, generic message-result helper is required

The old `deepx_msglp::Loop`, unrelated tools, session persistence architecture, and other crates are not refactored. Any remaining issues there are reported after completion.

## Accepted Decisions

1. Multiple `ask_user` tool calls in one model round are handled as a **sequential queue**.
2. A single ask with multiple questions is displayed as one Batch form and submitted atomically.
3. The next LLM request starts only after all asks and permission requests from the current tool round are resolved.
4. `ask_id` is the original tool-call ID. No synthetic `"0"` identifier is allowed.
5. An ask prompt is not a tool result. Only the final structured answer is written as that tool call's single result.
6. `UserInput` never implicitly answers a suspended ask. Only `AskResponse` or `AskDismiss` may resolve it.
7. Dismissing an ask aborts the suspended turn and leaves the next normal user input untouched.
8. Question answers are required, single-valued strings. Multi-select is not part of this protocol version.
9. For model input, presentation mode is derived: one normalized question is `Single`; two or more are `Batch`.

## Rejected Alternatives

### Merge all independent tool calls into one protocol ask

This gives one form but obscures the one-tool-call/one-tool-result boundary and makes partial validation and replay harder.

### Reject multiple ask tool calls

This is simpler but does not meet the requirement to support multiple asks without prematurely resuming the loop.

### Keep parsing ask prompts from `ToolResults`

This repeats the current root cause: the prompt occupies the only result slot before the user has answered and loses the tool-call identity at the UI boundary.

## Core Invariants

The implementation must preserve all of these invariants:

1. Each `ask_user` tool-call ID receives zero results while pending and exactly one result when answered.
2. `AskResponse.ask_id` must equal the currently displayed queued ask.
3. Every expected question ID appears exactly once in a response; unknown, duplicate, missing, and empty answers are rejected.
4. Rejected responses do not consume or mutate the active ask.
5. A resolved ask cannot be resolved again.
6. The ask queue preserves assistant tool-call order.
7. No gate request is made while unresolved permission calls or asks remain.
8. The frontend removes a prompt only after an agent acknowledgement, not merely after writing to stdin.
9. Cancel, dismiss, new session, resume session, undo, and process failure cannot leave an active frontend prompt pointing to discarded backend state.
10. `TurnEnd` and `Done` are emitted once per completed or aborted turn.

## Component Boundaries

### deepx-tools

`deepx-tools` owns model-facing ask schema and pure normalization. It exports a small ask-domain API without depending on `deepx-proto`:

```rust
pub struct NormalizedAsk {
    pub mode: NormalizedAskMode,
    pub questions: Vec<NormalizedAskQuestion>,
}

pub struct NormalizedAskQuestion {
    pub id: String,
    pub question: String,
    pub options: Vec<String>,
    pub allow_custom: bool,
}

pub fn normalize_ask_user(args: &serde_json::Value) -> Result<NormalizedAsk, AskUserError>;
```

Normalization rules:

- Accept the legacy `question/options/allow_custom` shape and the `questions` array shape.
- Generate `q1`, `q2`, ... only when an ID is omitted.
- Reject empty question arrays, blank question text, duplicate IDs, duplicate options, and a question with no options and `allow_custom=false`.
- Derive Single/Batch from normalized question count.
- Preserve question and option order.
- The registered handler may serialize validation output for direct tool diagnostics, but the Ring path must not execute a valid ask into the message store before an answer exists.

### deepx-proto

`deepx-proto` owns wire types only. The per-seed Tauri event channel provides session identity; every ask additionally carries turn and round identity for replay diagnostics.

```rust
Agent2Ui::AskUser {
    turn_id: String,
    round_num: u32,
    ask_id: String,
    mode: AskMode,
    questions: Vec<AskQuestion>,
}

Ui2Agent::AskResponse {
    ask_id: String,
    answers: Vec<AskAnswer>,
}

Ui2Agent::AskDismiss {
    ask_id: String,
}

Agent2Ui::AskResolved {
    ask_id: String,
    resolution: AskResolution, // answered | dismissed
}

Agent2Ui::AskRejected {
    ask_id: String,
    message: String,
}
```

`AskQuestion.id` is unique within one ask. `AskAnswer.answer` is either an exact option string or non-empty custom text when `allow_custom=true`. The protocol contains no legacy `answer` scalar and no hidden `[USER_QUERY]` payload.

### deepx-msglp new Ring

The new Ring owns lifecycle state. A suspended turn stores all remaining work needed to resume safely:

```rust
pub struct PendingAsk {
    pub call_id: String,
    pub mode: AskMode,
    pub questions: Vec<AskQuestion>,
}

pub struct TurnState {
    pub session_id: String,
    pub turn_id: String,
    pub round_num: u32,
    pub usage: Option<UsageInfo>,
    pub pending_permission_ids: Vec<String>,
    pub pending_asks: VecDeque<PendingAsk>,
    pub reason: YieldReason,
}
```

`ToolEngine::admit_batch` remains the authorization boundary. After bridge admission:

- Normal authorized tools become executable work.
- An authorized `ask_user` is normalized into `PendingAsk`, not executed as a worker.
- Invalid asks receive an immediate failed tool result and do not suspend.
- Permission-required calls remain in `ToolEngine` and are referenced by the suspended turn.

This avoids bypassing tool allowlists or bridge admission while preventing a prompt placeholder from occupying the tool-result slot.

## Authoritative Data Flow

### One ask with multiple questions

1. Gate returns an `ask_user` tool call with two or more questions.
2. Tool admission succeeds and normalization derives `Batch`.
3. The Ring executes other authorized tools from the round.
4. The Ring stores a suspended turn and emits `AskUser` for the queue front.
5. The frontend displays all questions and submits a complete answer set.
6. The Ring validates identity and all answers without consuming state first.
7. The Ring writes one structured result under the original tool-call ID:

```json
{"status":"answered","answers":[{"question_id":"q1","answer":"A"},{"question_id":"q2","answer":"custom"}]}
```

8. The Ring emits `AskResolved { resolution: answered }`.
9. With no other asks or permissions remaining, the Ring emits the completed tool round once and starts the next gate lap.

### Multiple ask calls in one round

1. Each admitted call becomes one `PendingAsk` in assistant tool-call order.
2. Only the queue front is emitted.
3. A valid response writes that call's answer result and acknowledges it.
4. If another ask remains, the Ring emits it immediately and stays suspended; no gate call occurs.
5. After the last ask is answered, the Ring resumes the next gate lap with one result for every original ask call.

### Mixed permissions and asks

1. Authorized normal tools execute as usual.
2. If any LLM tool approval remains, the turn first yields as `PermissionPending`; asks are retained but not displayed.
3. Each permission response updates `ToolEngine` and the suspended turn.
4. When the last approval resolves, the Ring either emits the first queued ask or resumes the gate if no asks remain.
5. This path also repairs the current new-Ring bug where `PermissionResolved` has no caller.

## Response Validation

Validation occurs before `TurnEngine` removes or advances the active ask:

- No suspended ask: emit `AskRejected` and do nothing else.
- Wrong or stale `ask_id`: emit `AskRejected`; keep the current ask active.
- Duplicate response question IDs: reject.
- Missing or unknown question IDs: reject.
- Blank answer: reject.
- Answer equals a configured option: accept.
- Otherwise accept only when `allow_custom=true`.
- Reorder accepted answers into the original question order before serialization.

Validation failure never invokes the model and never removes the frontend form.

## Dismiss, Cancel, and Session Semantics

### AskDismiss

- Validate `ask_id` against the queue front.
- Clear the suspended turn and all tool approvals belonging to it.
- Remove or finalize the incomplete assistant step using the same tested message-store invariant as explicit cancellation.
- Emit `AskResolved { resolution: dismissed }`, then one cancellation/end sequence.
- The next `UserInput` starts a new turn.

### Explicit Cancel

- Reset `TurnEngine`, `ToolEngine`, pending ask state, and frontend ask state.
- Emit one `Cancelled` and one terminal completion signal.

### Session switch, new session, undo, and agent death

- Backend session transitions reset session-scoped turn/tool state.
- Frontend ask queues are per ChatStore and cleared only for the affected seed.
- A stale response sent after a transition is rejected and cannot resume a new turn.
- Pending asks are not persisted across agent-process death; the interrupted turn is canceled and the UI reports the lost agent instead of pretending the answer was accepted.

## Frontend State Machine

Each ChatStore owns an ordered queue and transport status:

```typescript
type AskState = {
  askId: string;
  turnId: string;
  roundNum: number;
  mode: AskMode;
  questions: AskQuestion[];
  submitting: boolean;
  error?: string;
};
```

Rules:

- `AskUser`: deduplicate by `askId`; enqueue different asks.
- Only the queue front is rendered.
- Draft answers are keyed by `askId` and reset when that ID changes.
- Single option click may submit immediately.
- Batch submit is disabled until every question has a non-empty answer.
- Typing custom text updates the answer immediately; Enter is optional.
- Submitting sets `submitting=true` but keeps the form visible.
- `AskResolved`: remove the matching queue entry and reveal the next.
- `AskRejected`: keep the entry, clear `submitting`, and show the message.
- Command transport failure follows the same rejected state locally.
- Cancel/session cleanup clears only that store's queue and drafts.
- `ToolResults` never opens an ask dialog.

The UI continues using `AskDialog` for one question and `AskForm` for multiple questions. Both use the same store-driven lifecycle and validation helpers. No unrelated UI architecture is changed.

## Event Ordering and Idempotency

- The agent event channel preserves emission order: `AskResolved` precedes the next `AskUser` or the resumed round.
- Repeated `AskUser` with the same ID is idempotent in the frontend.
- Repeated `AskResponse` is rejected after the first acceptance and cannot start another gate lap.
- `ToolResults`, `TurnEnd`, and `Done` are emitted once at their owning layer. `TurnEngine` does not emit `TurnEnd` if `Loop::apply_outcome` owns it.

## Testing Strategy

### deepx-tools unit tests

- Legacy single question normalizes successfully.
- Multi-question input derives Batch even when mode is omitted.
- Duplicate IDs, duplicate options, blank questions, empty arrays, and unanswerable questions fail.
- Auto-generated IDs and custom-answer flags are stable.

### deepx-proto tests

- Round trips for `AskUser`, `AskResponse`, `AskDismiss`, `AskResolved`, and `AskRejected`.
- Generated TypeScript bindings match the Rust DTOs.
- The legacy scalar `AskResponse { answer }` shape is rejected.

### Production new-Ring integration tests

Tests instantiate `deepx_msglp::new::loop_core::Loop`, not the legacy public Loop, and use sequential mock SSE responses.

- One single ask: no second gate before answer; second request contains the exact structured answer.
- One batch ask: selecting only the first question does not resume; complete answers resume once.
- Multiple ask calls: first answer emits second ask without a gate call; last answer resumes exactly once.
- Wrong ask ID, partial answers, unknown IDs, empty answers, and duplicate responses do not advance the queue.
- Dismiss followed by normal input starts a new turn immediately.
- Cancel, new session, resume session, and undo invalidate pending ask IDs.
- Mixed permission and ask work resumes in the required order.
- Exactly one `ToolResults`, `TurnEnd`, and `Done` is emitted.
- The second gate request contains user answers and never contains the original prompt JSON as the answer result.

### Frontend tests

Add a dev-only Vitest/jsdom test setup scoped to `deepx-tauri`.

- Batch custom text submits without Enter.
- Incomplete Batch cannot submit.
- Option/custom switching sends the visible value.
- Ask drafts reset by ask ID and do not cross ChatStores.
- Duplicate events are idempotent.
- Transport failure and `AskRejected` preserve the form.
- `AskResolved` advances the queue.
- Dismiss and session cleanup remove stale prompts.

### Build and runtime gates

- Targeted Rust tests for tools, protocol, message store, and new-Ring lifecycle.
- `cargo check -p deepx-msglp`.
- `cargo check -p deepx-tauri`.
- Frontend typecheck, tests, and production build.
- A real `deepx-tauri.exe --agent` mock-SSE smoke test confirming the production entry point.
- `git diff --check`, `git diff --stat`, and `git status --short` to prevent unrelated formatting churn.

## Completion Criteria

The feature is complete only when all of the following are proven:

1. A single multi-question ask cannot resume after only the first answer.
2. Multiple ask calls are presented sequentially and resume the model only after the final answer.
3. The model receives the exact accepted answers under the correct original tool-call IDs.
4. Direct `Agent2Ui::AskUser` events are the only production prompt path.
5. Wrong, late, duplicate, missing, and malformed responses cannot consume another ask.
6. Dismiss and cancel cannot swallow the next user message.
7. Per-session frontend state cannot leak ask drafts or identities.
8. Permission suspension resumes correctly and terminal events are not duplicated.
9. Targeted tests and production-path smoke tests pass with no unreviewed workspace changes.

## Deferred Findings Report

The final implementation report must separately classify remaining work as:

- other-crate refactoring worth scheduling,
- `ask_user` tool-only hardening,
- new Ring hardening,
- legacy Loop removal or migration.

Deferred findings are recommendations, not hidden completion requirements, unless testing shows they directly violate an invariant above.
