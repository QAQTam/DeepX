# ask_user Lifecycle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `ask_user` a production-ready, identity-safe pause/resume operation for single-question, batch-question, and multiple-call rounds in the new Ring and Tauri desktop.

**Architecture:** `deepx-tools` provides pure normalization, `deepx-proto` provides explicit request/response/acknowledgement DTOs, and the production new Ring owns a suspended-turn queue keyed by original tool-call IDs. The frontend keeps a per-ChatStore queue and removes prompts only after `AskResolved`; `ToolResults` never acts as an ask transport.

**Tech Stack:** Rust 2024, serde/serde_json, ts-rs, `deepx_msglp::new::loop_core::Loop`, tiny_http mock SSE tests, SolidJS 1.9, Tauri 2, Vitest 4.1.6, jsdom, pnpm 11.

## Global Constraints

- Multiple independent `ask_user` calls in one model round use a sequential queue in assistant tool-call order.
- One ask with two or more questions is submitted atomically; a partial answer set cannot resume the model.
- `ask_id` is the original tool-call ID; synthetic IDs and `UserInput` fallback answers are forbidden.
- Each ask call has zero tool results while pending and exactly one structured result after acceptance.
- The next gate request is forbidden while a permission request or queued ask from the current round is unresolved.
- Dismissal aborts only the suspended turn and cannot consume the next normal user input.
- The old Loop and unrelated crates remain unchanged unless a failing production-path test proves a direct dependency.
- Avoid workspace-wide formatting; run file-scoped rustfmt only on touched Rust files and inspect `git diff --stat` afterward.
- Preserve unrelated dirty-worktree changes; stage only files listed by the current task.

---

### Task 1: Pure ask normalization in deepx-tools

**Files:**
- Modify: `crates/deepx-tools/src/ask_user.rs`

**Interfaces:**
- Consumes: model arguments in either legacy `{question, options, allow_custom}` form or the `questions` array form.
- Produces: `pub fn normalize_ask_user(&serde_json::Value) -> Result<NormalizedAsk, AskUserError>` and the public `NormalizedAsk`, `NormalizedAskMode`, `NormalizedAskQuestion`, and `AskUserError` types.

- [ ] **Step 1: Replace the current permissive tests with failing normalization tests**

Add tests that call the public normalizer directly:

```rust
#[test]
fn multi_question_input_derives_batch_without_mode() {
    let ask = normalize_ask_user(&serde_json::json!({
        "questions": [
            {"id":"arch", "question":"Architecture?", "options":["A","B"], "allow_custom":false},
            {"question":"Strategy?", "allow_custom":true}
        ]
    })).unwrap();
    assert_eq!(ask.mode, NormalizedAskMode::Batch);
    assert_eq!(ask.questions[0].id, "arch");
    assert_eq!(ask.questions[1].id, "q2");
}

#[test]
fn duplicate_ids_are_rejected() {
    let error = normalize_ask_user(&serde_json::json!({
        "questions": [
            {"id":"same", "question":"First?", "allow_custom":true},
            {"id":"same", "question":"Second?", "allow_custom":true}
        ]
    })).unwrap_err();
    assert_eq!(error.code, "DUPLICATE_QUESTION_ID");
}

#[test]
fn duplicate_options_are_rejected() {
    let error = normalize_ask_user(&serde_json::json!({
        "question":"Pick one", "options":["A","A"], "allow_custom":false
    })).unwrap_err();
    assert_eq!(error.code, "DUPLICATE_OPTION");
}

#[test]
fn unanswerable_question_is_rejected() {
    let error = normalize_ask_user(&serde_json::json!({
        "question":"Blocked", "options":[], "allow_custom":false
    })).unwrap_err();
    assert_eq!(error.code, "UNANSWERABLE_QUESTION");
}
```

- [ ] **Step 2: Run the focused tests and verify RED**

Run: `cargo test -p deepx-tools ask_user::tests -- --nocapture`

Expected: compilation fails because `normalize_ask_user`, `NormalizedAskMode`, and `AskUserError` do not yet provide the required public API; the failure is not a syntax or fixture error.

- [ ] **Step 3: Implement the pure normalizer and make direct execution diagnostic-only**

Use these exact public shapes and validation order:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NormalizedAskMode { Single, Batch }

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct NormalizedAskQuestion {
    pub id: String,
    pub question: String,
    pub options: Vec<String>,
    pub allow_custom: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct NormalizedAsk {
    pub mode: NormalizedAskMode,
    pub questions: Vec<NormalizedAskQuestion>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskUserError {
    pub code: &'static str,
    pub message: String,
}

pub fn normalize_ask_user(args: &serde_json::Value) -> Result<NormalizedAsk, AskUserError> {
    let raw_questions = match args.get("questions") {
        Some(serde_json::Value::Array(items)) => items.clone(),
        Some(_) => return Err(AskUserError { code: "INVALID_QUESTIONS", message: "questions must be an array".into() }),
        None => vec![serde_json::json!({
            "id": "q1",
            "question": args.get("question").and_then(|v| v.as_str()).unwrap_or(""),
            "options": args.get("options").cloned().unwrap_or_else(|| serde_json::json!([])),
            "allow_custom": args.get("allow_custom").and_then(|v| v.as_bool()).unwrap_or(true)
        })],
    };
    if raw_questions.is_empty() {
        return Err(AskUserError { code: "EMPTY_QUESTIONS", message: "at least one question is required".into() });
    }
    let mut ids = std::collections::HashSet::new();
    let mut questions = Vec::with_capacity(raw_questions.len());
    for (index, raw) in raw_questions.iter().enumerate() {
        let question = raw.get("question").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if question.trim().is_empty() {
            return Err(AskUserError { code: "MISSING_QUESTION", message: format!("questions[{index}].question is required") });
        }
        let id = raw.get("id").and_then(|v| v.as_str()).filter(|id| !id.trim().is_empty())
            .map(str::to_string).unwrap_or_else(|| format!("q{}", index + 1));
        if !ids.insert(id.clone()) {
            return Err(AskUserError { code: "DUPLICATE_QUESTION_ID", message: format!("duplicate question id: {id}") });
        }
        let options = raw.get("options").and_then(|v| v.as_array()).map(|items| {
            items.iter().filter_map(|item| item.as_str().map(str::to_string)).collect::<Vec<_>>()
        }).unwrap_or_default();
        let mut unique_options = std::collections::HashSet::new();
        if options.iter().any(|option| !unique_options.insert(option.clone())) {
            return Err(AskUserError { code: "DUPLICATE_OPTION", message: format!("question {id} contains duplicate options") });
        }
        let allow_custom = raw.get("allow_custom").and_then(|v| v.as_bool()).unwrap_or(true);
        if options.is_empty() && !allow_custom {
            return Err(AskUserError { code: "UNANSWERABLE_QUESTION", message: format!("question {id} has no valid answer path") });
        }
        questions.push(NormalizedAskQuestion { id, question, options, allow_custom });
    }
    let mode = if questions.len() == 1 { NormalizedAskMode::Single } else { NormalizedAskMode::Batch };
    Ok(NormalizedAsk { mode, questions })
}
```

`exec_ask_user` must serialize this normalized object through `json_ok` and serialize errors through `json_err`; it must not prefix `[USER_QUERY]` or emit `user_query=true`. Update the registered JSON schema so `questions` and legacy `question` are the two accepted roots, and do not expose a caller-controlled presentation mode.

- [ ] **Step 4: Run the focused tools tests and verify GREEN**

Run: `cargo test -p deepx-tools ask_user::tests -- --nocapture`

Expected: all ask normalization and schema tests pass; no test expects `[USER_QUERY]` or `user_query=true`.

- [ ] **Step 5: Commit the tools normalization slice**

```powershell
git add -- crates/deepx-tools/src/ask_user.rs
git commit -m "refactor(tools): normalize ask_user arguments"
```

---

### Task 2: Explicit ask protocol and generated TypeScript bindings

**Files:**
- Modify: `crates/deepx-proto/src/agent_protocol.rs`
- Modify: `crates/deepx-proto/src/lib.rs`
- Regenerate: `crates/deepx-tauri/src/lib/types/Agent2Ui.ts`
- Regenerate: `crates/deepx-tauri/src/lib/types/Ui2Agent.ts`
- Create: `crates/deepx-tauri/src/lib/types/AskAnswer.ts`
- Create: `crates/deepx-tauri/src/lib/types/AskMode.ts`
- Create: `crates/deepx-tauri/src/lib/types/AskQuestion.ts`
- Create: `crates/deepx-tauri/src/lib/types/AskResolution.ts`
- Modify: `crates/deepx-tauri/src/lib/types/index.ts`

**Interfaces:**
- Consumes: normalized ask content and original tool-call identity from the new Ring.
- Produces: direct `AskUser`, `AskResponse`, `AskDismiss`, `AskResolved`, and `AskRejected` wire messages.

- [ ] **Step 1: Add failing Rust round-trip tests for the final wire contract**

```rust
#[test]
fn ask_user_round_trip_preserves_turn_and_call_identity() {
    let event = Agent2Ui::AskUser {
        turn_id: "t7".into(),
        round_num: 3,
        ask_id: "call-ask-1".into(),
        mode: AskMode::Batch,
        questions: vec![AskQuestion {
            id: "q1".into(), question: "Choose".into(), options: vec!["A".into()], allow_custom: true,
        }],
    };
    let json = serde_json::to_string(&event).unwrap();
    let decoded: Agent2Ui = serde_json::from_str(&json).unwrap();
    assert!(matches!(decoded, Agent2Ui::AskUser { turn_id, round_num: 3, ask_id, .. }
        if turn_id == "t7" && ask_id == "call-ask-1"));
}

#[test]
fn ask_acknowledgements_round_trip() {
    for event in [
        Agent2Ui::AskResolved { ask_id: "a1".into(), resolution: AskResolution::Answered },
        Agent2Ui::AskRejected { ask_id: "a1".into(), message: "stale ask_id".into() },
    ] {
        let json = serde_json::to_string(&event).unwrap();
        serde_json::from_str::<Agent2Ui>(&json).unwrap();
    }
}

#[test]
fn legacy_scalar_answer_is_rejected() {
    assert!(serde_json::from_str::<Ui2Agent>(r#"{"type":"ask_response","answer":"A"}"#).is_err());
}
```

- [ ] **Step 2: Run protocol tests and verify RED**

Run: `cargo test -p deepx-proto agent_protocol::tests -- --nocapture`

Expected: compilation fails because `turn_id`, `round_num`, `AskResolution`, `AskResolved`, and `AskRejected` are absent.

- [ ] **Step 3: Implement the protocol DTOs**

Use these exact variants and derive `Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS` on ask leaf types:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AskMode { Single, Batch }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AskResolution { Answered, Dismissed }

Ui2Agent::AskResponse { ask_id: String, answers: Vec<AskAnswer> }
Ui2Agent::AskDismiss { ask_id: String }

Agent2Ui::AskUser {
    turn_id: String,
    round_num: u32,
    ask_id: String,
    mode: AskMode,
    questions: Vec<AskQuestion>,
}
Agent2Ui::AskResolved { ask_id: String, resolution: AskResolution }
Agent2Ui::AskRejected { ask_id: String, message: String }
```

Re-export `AskAnswer`, `AskMode`, `AskQuestion`, and `AskResolution` from `deepx-proto/src/lib.rs`. Do not add legacy scalar fields or a hidden prompt payload.

- [ ] **Step 4: Verify Rust protocol tests and regenerate bindings**

```powershell
cargo test -p deepx-proto agent_protocol::tests -- --nocapture
cargo test -p deepx-proto
Copy-Item crates/deepx-proto/bindings/Agent2Ui.ts,crates/deepx-proto/bindings/Ui2Agent.ts,crates/deepx-proto/bindings/AskAnswer.ts,crates/deepx-proto/bindings/AskMode.ts,crates/deepx-proto/bindings/AskQuestion.ts,crates/deepx-proto/bindings/AskResolution.ts crates/deepx-tauri/src/lib/types/ -Force
```

Expected: protocol tests pass and generated TypeScript has all five event variants with no scalar `answer` compatibility branch.

- [ ] **Step 5: Commit the protocol slice**

```powershell
git add -- crates/deepx-proto/src/agent_protocol.rs crates/deepx-proto/src/lib.rs crates/deepx-tauri/src/lib/types
git commit -m "feat(proto): define explicit ask lifecycle events"
```

---

### Task 3: Production new-Ring lifecycle regression harness

**Files:**
- Create: `crates/deepx-msglp/tests/ask_user_lifecycle.rs`

**Interfaces:**
- Consumes: `deepx_msglp::Loop`, `Ui2Agent`, `Agent2Ui`, and sequential OpenAI-compatible SSE responses.
- Produces: executable proof for batch atomicity, sequential independent asks, identity validation, exact tool results, and dismissal behavior.

- [ ] **Step 1: Create the mock-SSE harness through the real public Loop**

Copy the concrete `MockServer`, `tool_round`, `final_round`, `send`, `expect_event`, `collect_through_done`, and `assert_single_completion` helpers from `crates/deepx-msglp/tests/permission_lifecycle.rs`. Keep `use deepx_msglp::Loop;` so the test follows the production new-Ring export, and configure `provider_id` and `endpoint` to empty strings exactly as the permission lifecycle test does. Give `run_case` this signature and pass a clone of `MockServer.requests` into the driver closure:

```rust
fn run_case(
    permission_level: u8,
    workspace: &std::path::Path,
    scenarios: Vec<Vec<String>>,
    expected_requests: usize,
    test: impl FnOnce(
        &mut os_pipe::PipeWriter,
        &std::sync::mpsc::Receiver<Agent2Ui>,
        Arc<AtomicUsize>,
    ) + Send + 'static,
) -> Vec<String> {
    SESSION_INIT.call_once(|| {
        deepx_session::SessionManager::init(deepx_types::platform::data_dir(), false);
    });
    let mock = MockServer::sequential(scenarios);
    let request_count = mock.requests.clone();
    deepx_tools::set_workspace(&workspace.to_string_lossy());
    let mut agent = AgentState::init("ask-lifecycle-test");
    agent.ephemeral = true;
    agent.config.permission_level = permission_level;
    agent.config.base_url = mock.base_url.clone();
    agent.config.api_key = "sk-test".into();
    agent.config.model = "test-model".into();
    agent.config.provider_id.clear();
    agent.config.endpoint.clear();
    agent.config.compliance_enabled = false;
    let (input_reader, mut input_writer) = os_pipe::pipe().unwrap();
    let (output_reader, output_writer) = os_pipe::pipe().unwrap();
    let mut agent_loop = Loop::new_ipc(agent, BufReader::new(input_reader), output_writer);
    let (event_tx, event_rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(output_reader).lines().map_while(Result::ok) {
            if let Ok(event) = serde_json::from_str::<Agent2Ui>(&line) {
                if event_tx.send(event).is_err() { break; }
            }
        }
    });
    let workspace = workspace.to_path_buf();
    let driver = thread::spawn(move || {
        send(&mut input_writer, Ui2Agent::CreateSession);
        expect_event(&event_rx, Duration::from_secs(5), |event| matches!(event, Agent2Ui::SessionCreated { .. }));
        deepx_tools::set_workspace(&workspace.to_string_lossy());
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            test(&mut input_writer, &event_rx, request_count)
        }));
        send(&mut input_writer, Ui2Agent::Shutdown);
        if let Err(payload) = outcome { std::panic::resume_unwind(payload); }
    });
    agent_loop.run();
    driver.join().expect("test driver");
    assert_eq!(mock.requests.load(Ordering::SeqCst), expected_requests);
    mock.bodies.lock().expect("body lock").clone()
}

fn expect_ask(
    receiver: &std::sync::mpsc::Receiver<Agent2Ui>,
    expected_id: &str,
    expected_questions: usize,
) -> Agent2Ui {
    expect_event(receiver, Duration::from_secs(5), |event| matches!(
        event,
        Agent2Ui::AskUser { ask_id, questions, .. }
            if ask_id == expected_id && questions.len() == expected_questions
    ))
}

fn expect_rejected(receiver: &std::sync::mpsc::Receiver<Agent2Ui>, expected_id: &str) {
    expect_event(receiver, Duration::from_secs(5), |event| matches!(
        event, Agent2Ui::AskRejected { ask_id, .. } if ask_id == expected_id
    ));
}

fn expect_resolved(
    receiver: &std::sync::mpsc::Receiver<Agent2Ui>,
    expected_id: &str,
    expected_resolution: AskResolution,
) {
    expect_event(receiver, Duration::from_secs(5), |event| matches!(
        event,
        Agent2Ui::AskResolved { ask_id, resolution }
            if ask_id == expected_id && *resolution == expected_resolution
    ));
}

fn answer(ask_id: &str, question_id: &str, value: &str) -> Ui2Agent {
    Ui2Agent::AskResponse {
        ask_id: ask_id.into(),
        answers: vec![AskAnswer { question_id: question_id.into(), answer: value.into() }],
    }
}
```

- [ ] **Step 2: Add the failing batch-atomicity test**

```rust
#[test]
fn batch_ask_waits_for_every_answer_and_writes_one_exact_result() {
    let temp = tempfile::tempdir().unwrap();
    let bodies = run_case(
        1,
        temp.path(),
        vec![
            tool_round(&[("ask-batch", "ask_user", json!({"questions":[
                {"id":"q1","question":"First?","options":["A","B"],"allow_custom":false},
                {"id":"q2","question":"Second?","options":["C","D"],"allow_custom":false}
            ]}))]),
            final_round("finished"),
        ],
        2,
        |writer, receiver, request_count| {
            send(writer, Ui2Agent::UserInput { text: "ask me".into() });
            let ask = expect_ask(receiver, "ask-batch", 2);
            assert!(matches!(ask, Agent2Ui::AskUser { mode: AskMode::Batch, .. }));
            send(writer, Ui2Agent::AskResponse {
                ask_id: "ask-batch".into(),
                answers: vec![AskAnswer { question_id: "q1".into(), answer: "A".into() }],
            });
            expect_rejected(receiver, "ask-batch");
            assert_eq!(request_count.load(Ordering::SeqCst), 1);
            send(writer, Ui2Agent::AskResponse {
                ask_id: "ask-batch".into(),
                answers: vec![
                    AskAnswer { question_id: "q1".into(), answer: "A".into() },
                    AskAnswer { question_id: "q2".into(), answer: "D".into() },
                ],
            });
            expect_resolved(receiver, "ask-batch", AskResolution::Answered);
            assert_single_completion(&collect_through_done(receiver), 1);
        },
    );
    assert!(bodies[1].contains("\"tool_call_id\":\"ask-batch\""));
    assert!(bodies[1].contains("\\\"question_id\\\":\\\"q1\\\""));
    assert!(bodies[1].contains("\\\"answer\\\":\\\"D\\\""));
    assert!(!bodies[1].contains("First?"));
}
```

- [ ] **Step 3: Add failing multiple-call, stale-response, and dismiss tests**

The multiple-call test must assert this exact sequence:

```rust
send(writer, Ui2Agent::UserInput { text: "ask twice".into() });
expect_ask(receiver, "ask-1", 1);
send(writer, answer("ask-1", "q1", "A"));
expect_resolved(receiver, "ask-1", AskResolution::Answered);
expect_ask(receiver, "ask-2", 1);
assert_eq!(request_count.load(Ordering::SeqCst), 1);
send(writer, answer("ask-2", "q1", "B"));
expect_resolved(receiver, "ask-2", AskResolution::Answered);
assert_single_completion(&collect_through_done(receiver), 2);
```

The stale-response and dismiss tests use these concrete command sequences:

```rust
send(writer, Ui2Agent::AskResponse {
    ask_id: "old-id".into(),
    answers: vec![AskAnswer { question_id:"q1".into(), answer:"A".into() }],
});
expect_rejected(receiver, "old-id");
assert_eq!(request_count.load(Ordering::SeqCst), 1);
send(writer, answer("active-id", "q1", "A"));
expect_resolved(receiver, "active-id", AskResolution::Answered);

send(writer, Ui2Agent::AskDismiss { ask_id:"dismiss-id".into() });
expect_resolved(receiver, "dismiss-id", AskResolution::Dismissed);
let aborted = collect_through_done(receiver);
assert_eq!(aborted.iter().filter(|event| matches!(event, Agent2Ui::TurnEnd { .. })).count(), 1);
assert_eq!(aborted.iter().filter(|event| matches!(event, Agent2Ui::Done)).count(), 1);
send(writer, Ui2Agent::UserInput { text:"fresh turn".into() });
expect_event(receiver, Duration::from_secs(5), |event| matches!(event, Agent2Ui::RoundComplete { .. }));
```

- [ ] **Step 4: Run the new integration target and verify RED**

Run: `cargo test -p deepx-msglp --test ask_user_lifecycle -- --test-threads=1 --nocapture`

Expected: failures show missing direct `AskUser` events and/or premature gate requests; the harness itself creates a session and reaches the first model tool round.

- [ ] **Step 5: Commit the RED tests only**

```powershell
git add -- crates/deepx-msglp/tests/ask_user_lifecycle.rs
git commit -m "test(msglp): reproduce ask lifecycle failures"
```

---

### Task 4: Ask admission, suspended queue, validation, and acknowledgement in the new Ring

**Files:**
- Modify: `crates/deepx-msglp/src/new/types.rs`
- Modify: `crates/deepx-msglp/src/new/engine_tool.rs`
- Modify: `crates/deepx-msglp/src/new/engine_turn.rs`
- Modify: `crates/deepx-msglp/src/new/loop_core.rs`

**Interfaces:**
- Consumes: authorized `ask_user` calls from `ToolEngine::admit_batch` and typed responses from `Ui2Agent`.
- Produces: ordered `PendingAsk` values, direct ask events, a single structured tool result per answer, and no gate lap before the queue is empty.

- [ ] **Step 1: Add the suspended-turn queue types**

```rust
use std::collections::VecDeque;
use deepx_proto::{Agent2Ui, AskMode, AskQuestion};

#[derive(Debug, Clone)]
pub struct PendingAsk {
    pub call_id: String,
    pub mode: AskMode,
    pub questions: Vec<AskQuestion>,
}

pub struct TurnState {
    pub turn_id: String,
    pub round_num: u32,
    pub usage: Option<UsageInfo>,
    pub pending_permission_ids: Vec<String>,
    pub pending_asks: VecDeque<PendingAsk>,
    pub session_id: String,
    pub reason: YieldReason,
}
```

Rename every `pending_call_ids` reference to `pending_permission_ids` so the two pending domains cannot be confused.

- [ ] **Step 2: Make ToolEngine return admitted asks without executing them**

Define one batch result:

```rust
pub struct BatchAdmission {
    pub authorized: Vec<AdmittedTool>,
    pub pending_permission_ids: Vec<String>,
    pub pending_asks: VecDeque<PendingAsk>,
}
```

Inside the existing `Admission::Authorized(auth)` arm, branch only after the bridge has authorized the invocation:

```rust
if auth.tool_name() == "ask_user" {
    match deepx_tools::ask_user::normalize_ask_user(auth.args()) {
        Ok(normalized) => pending_asks.push_back(PendingAsk {
            call_id: auth.call_id().to_string(),
            mode: match normalized.mode {
                deepx_tools::ask_user::NormalizedAskMode::Single => AskMode::Single,
                deepx_tools::ask_user::NormalizedAskMode::Batch => AskMode::Batch,
            },
            questions: normalized.questions.into_iter().map(|question| AskQuestion {
                id: question.id,
                question: question.question,
                options: question.options,
                allow_custom: question.allow_custom,
            }).collect(),
        }),
        Err(error) => ctx.agent.msg.push_tool_result_direct(
            auth.call_id(),
            &serde_json::json!({"status":"error","code":error.code,"message":error.message}).to_string(),
            false,
        ),
    }
} else {
    authorized.push(AdmittedTool { call_id: tool.id.clone(), auth: Box::new(auth) });
}
```

This branch must remain after `deepx_tools::bridge::admit`; do not bypass allowlists or permission policy.

- [ ] **Step 3: Implement direct ask emission and response validation on TurnEngine**

Add these exact methods:

```rust
fn emit_active_ask(ctx: &mut RingContext, state: &TurnState) {
    if let Some(ask) = state.pending_asks.front() {
        ctx.emitter.emit(Agent2Ui::AskUser {
            turn_id: state.turn_id.clone(),
            round_num: state.round_num,
            ask_id: ask.call_id.clone(),
            mode: ask.mode,
            questions: ask.questions.clone(),
        });
    }
}

fn validate_answers(ask: &PendingAsk, answers: &[AskAnswer]) -> Result<Vec<AskAnswer>, String> {
    let mut supplied = std::collections::HashMap::new();
    for answer in answers {
        if supplied.insert(answer.question_id.as_str(), answer.answer.as_str()).is_some() {
            return Err(format!("duplicate answer for {}", answer.question_id));
        }
    }
    let mut ordered = Vec::with_capacity(ask.questions.len());
    for question in &ask.questions {
        let answer = supplied.remove(question.id.as_str()).ok_or_else(|| format!("missing answer for {}", question.id))?;
        if answer.trim().is_empty() {
            return Err(format!("empty answer for {}", question.id));
        }
        if !question.options.iter().any(|option| option == answer) && !question.allow_custom {
            return Err(format!("invalid answer for {}", question.id));
        }
        ordered.push(AskAnswer { question_id: question.id.clone(), answer: answer.to_string() });
    }
    if !supplied.is_empty() {
        return Err("response contains unknown question ids".into());
    }
    Ok(ordered)
}
```

`handle_ask_response` must inspect without consuming first; reject missing suspension, wrong reason, stale `ask_id`, incomplete answers, duplicates, unknown IDs, blank answers, and disallowed custom text by emitting `AskRejected`. On acceptance it must pop exactly one queue entry and write:

```rust
let content = serde_json::json!({"status":"answered","answers":ordered}).to_string();
ctx.agent.msg.push_tool_result_direct(&active.call_id, &content, true);
ctx.emitter.emit(Agent2Ui::AskResolved {
    ask_id: active.call_id,
    resolution: AskResolution::Answered,
});
```

If another ask remains, emit it and return `YieldToUser` without calling the gate. If the queue is empty, emit the completed tool round once and call `run_lap` for `round_num + 1`.

- [ ] **Step 4: Replace prompt-result detection with queue suspension**

In `run_lap`, use `BatchAdmission` from `admit_batch`. After normal authorized tools execute:

```rust
if !admission.pending_permission_ids.is_empty() || !admission.pending_asks.is_empty() {
    let reason = if admission.pending_permission_ids.is_empty() {
        YieldReason::AskUser
    } else {
        YieldReason::PermissionPending
    };
    self.suspended = Some(TurnState {
        session_id: ctx.agent.session.seed.clone(),
        turn_id: turn_id.clone(),
        round_num,
        pending_permission_ids: admission.pending_permission_ids,
        pending_asks: admission.pending_asks,
        usage: last_usage.clone(),
        reason,
    });
    if reason == YieldReason::AskUser {
        Self::emit_active_ask(ctx, self.suspended.as_ref().unwrap());
    }
    return Outcome::YieldToUser { turn_id, reason };
}
```

Delete the scan for `[USER_QUERY]` and `user_query=true`. Remove `UserInput` from the AskUser suspension guard and from the fallback answer path.

- [ ] **Step 5: Route typed response and dismissal methods from Loop**

Replace the answer-joining branch with:

```rust
Ui2Agent::AskResponse { ask_id, answers } => {
    Some(self.session.turn.handle_ask_response(&mut ctx, &mut self.session.tool, ask_id, answers))
}
Ui2Agent::AskDismiss { ask_id } => {
    Some(self.session.turn.handle_ask_dismiss(&mut ctx, &mut self.session.tool, ask_id))
}
```

Do not set the global cancel flag merely because a dialog was dismissed. `handle_ask_dismiss` validates the active ID, clears the suspended state and its tool approvals, removes the incomplete assistant step, emits `AskResolved::Dismissed`, and returns a typed aborted-turn outcome handled once by `Loop::apply_outcome`.

Add this exact engine-to-loop outcome in `types.rs`:

```rust
Outcome::TurnAborted {
    turn_id: String,
    usage: Option<UsageInfo>,
}
```

`handle_ask_dismiss` returns the saved turn ID and usage in this outcome only after identity validation succeeds; a stale dismiss emits `AskRejected` and returns `Handled` without consuming the active queue.

- [ ] **Step 6: Run the lifecycle target and verify GREEN**

Run: `cargo test -p deepx-msglp --test ask_user_lifecycle -- --test-threads=1 --nocapture`

Expected: batch, sequential-call, stale-response, exact-result, and dismiss/fresh-input tests pass.

- [ ] **Step 7: Commit the new-Ring ask lifecycle slice**

```powershell
git add -- crates/deepx-msglp/src/new/types.rs crates/deepx-msglp/src/new/engine_tool.rs crates/deepx-msglp/src/new/engine_turn.rs crates/deepx-msglp/src/new/loop_core.rs
git commit -m "fix(msglp): queue and resume ask_user calls safely"
```

---

### Task 5: Permission handoff, terminal-event ownership, and reset races

**Files:**
- Modify: `crates/deepx-msglp/tests/permission_lifecycle.rs`
- Modify: `crates/deepx-msglp/tests/ask_user_lifecycle.rs`
- Modify: `crates/deepx-msglp/src/new/engine_tool.rs`
- Modify: `crates/deepx-msglp/src/new/engine_turn.rs`
- Modify: `crates/deepx-msglp/src/new/loop_core.rs`

**Interfaces:**
- Consumes: a resolved permission call ID and the same suspended `TurnState` that may also contain asks.
- Produces: exactly one continuation after the final approval, then the first ask if present; one owner for `TurnEnd` and `Done`.

- [ ] **Step 1: Add failing permission-plus-ask and reset tests**

Use this mixed-round test body:

```rust
#[test]
fn permission_finishes_before_queued_ask_and_gate_waits_for_both() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("input.txt");
    std::fs::write(&path, "hello").unwrap();
    let bodies = run_case(
        1,
        temp.path(),
        vec![
            tool_round(&[
                ("read-1", "read", json!({"path":path})),
                ("ask-1", "ask_user", json!({"question":"Continue?","options":["yes"],"allow_custom":false})),
            ]),
            final_round("finished"),
        ],
        2,
        |writer, receiver, request_count| {
            send(writer, Ui2Agent::UserInput { text:"read then ask".into() });
            expect_event(receiver, Duration::from_secs(5), |event| matches!(
                event, Agent2Ui::PermissionRequest { tool_call_id, .. } if tool_call_id == "read-1"
            ));
            assert_eq!(request_count.load(Ordering::SeqCst), 1);
            send(writer, Ui2Agent::PermissionResponse {
                tool_call_id:"read-1".into(), approved:true, trust_folder:false,
            });
            expect_ask(receiver, "ask-1", 1);
            assert_eq!(request_count.load(Ordering::SeqCst), 1);
            send(writer, answer("ask-1", "q1", "yes"));
            expect_resolved(receiver, "ask-1", AskResolution::Answered);
            assert_single_completion(&collect_through_done(receiver), 2);
        },
    );
    assert!(bodies[1].contains("\"tool_call_id\":\"read-1\""));
    assert!(bodies[1].contains("\"tool_call_id\":\"ask-1\""));
}
```

For each reset command, start an ask, record its `turn_id` and `ask_id`, send the reset, then send the stale response below and assert one `AskRejected` with no increment in the mock request counter:

```rust
let reset_commands = [
    Ui2Agent::Cancel,
    Ui2Agent::NewSession,
    Ui2Agent::ResumeSession { seed: existing_seed.clone() },
    Ui2Agent::UndoTurn { turn_id: active_turn_id.clone() },
];
send(writer, reset_command);
send(writer, Ui2Agent::AskResponse {
    ask_id: stale_ask_id.clone(),
    answers: vec![AskAnswer { question_id:"q1".into(), answer:"yes".into() }],
});
expect_rejected(receiver, &stale_ask_id);
assert_eq!(request_count.load(Ordering::SeqCst), requests_before_reset);
```

In `permission_lifecycle.rs`, retain and strengthen `assert_single_completion` so each completed turn has exactly one `ToolResults`, one `TurnEnd`, and one `Done`.

- [ ] **Step 2: Run both integration targets and verify RED**

```powershell
cargo test -p deepx-msglp --test permission_lifecycle -- --test-threads=1 --nocapture
cargo test -p deepx-msglp --test ask_user_lifecycle -- --test-threads=1 --nocapture
```

Expected: current permission response returns `Handled` without moving `TurnState`, and completed turns expose duplicate terminal emission before the fix.

- [ ] **Step 3: Return typed permission disposition and advance TurnEngine**

Define:

```rust
pub enum PermissionDisposition {
    Ignored,
    UiHandled,
    LlmResolved { call_id: String },
}
```

`ToolEngine::handle_permission_response` returns `LlmResolved` after approved, rejected, or expired LLM challenges have written their final result; unknown/replayed responses return `Ignored`. In Loop, pass `LlmResolved.call_id` to `TurnEngine::handle_permission_resolved`. That method removes only the matching `pending_permission_ids` entry. It returns `Handled` while approvals remain; after the final approval it either switches the saved reason to `AskUser` and emits the queue front, or emits the completed tool round and resumes the next lap.

- [ ] **Step 4: Give Loop sole terminal-event ownership**

Remove `Agent2Ui::TurnEnd` emission from the normal completion path in `TurnEngine::run_lap`. Keep `Loop::apply_outcome` as the only normal-turn owner:

```rust
Outcome::TurnComplete { turn_id, usage } => {
    self.phase = LoopPhase::Idle;
    let _ = self.event_tx.send(Agent2Ui::TurnEnd {
        turn_id,
        stop_reason: None,
        usage,
    });
    let _ = self.event_tx.send(Agent2Ui::Done);
}
```

For dismissed or explicitly cancelled suspended turns, add one typed aborted outcome and emit one `Cancelled`, one `TurnEnd`, and one `Done`; reset TurnEngine and ToolEngine before accepting another `UserInput`.

- [ ] **Step 5: Verify both lifecycle suites and crate check**

```powershell
cargo test -p deepx-msglp --test permission_lifecycle -- --test-threads=1 --nocapture
cargo test -p deepx-msglp --test ask_user_lifecycle -- --test-threads=1 --nocapture
cargo check -p deepx-msglp
```

Expected: all lifecycle tests pass, each completion count is one, and mixed permission/ask work does not enter the gate early.

- [ ] **Step 6: Commit permission and event-order hardening**

```powershell
git add -- crates/deepx-msglp/tests/permission_lifecycle.rs crates/deepx-msglp/tests/ask_user_lifecycle.rs crates/deepx-msglp/src/new/engine_tool.rs crates/deepx-msglp/src/new/engine_turn.rs crates/deepx-msglp/src/new/loop_core.rs
git commit -m "fix(msglp): resume mixed suspensions exactly once"
```

---

### Task 6: Tauri transport and per-ChatStore acknowledged queue

**Files:**
- Modify: `crates/deepx-tauri/src-tauri/src/agent_bridge.rs`
- Modify: `crates/deepx-tauri/src-tauri/src/lib.rs`
- Modify: `crates/deepx-tauri/src/App.tsx`
- Modify: `crates/deepx-tauri/src/store/chat.ts`
- Modify: `crates/deepx-tauri/src/components/ToolRow.tsx`
- Modify: `crates/deepx-tauri/package.json`
- Modify: `crates/deepx-tauri/pnpm-lock.yaml`
- Modify: `crates/deepx-tauri/vite.config.ts`
- Create: `crates/deepx-tauri/src/store/chat.ask.test.ts`

**Interfaces:**
- Consumes: direct protocol events routed to the ChatStore belonging to `listenerSeed`.
- Produces: a per-store FIFO prompt queue with `submitting` and `error` state; transport success alone never dequeues a prompt.

- [ ] **Step 1: Install the test runner and configure jsdom**

```powershell
pnpm --dir crates/deepx-tauri add -D vitest@4.1.6 jsdom
```

Add `"test": "vitest run"` to package scripts and merge this into the existing Vite config:

```ts
/// <reference types="vitest/config" />
export default defineConfig({
  test: {
    environment: "jsdom",
    clearMocks: true,
    restoreMocks: true,
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
```

Keep the existing Solid, Tailwind, alias, server, and build configuration fields in the same object.

- [ ] **Step 2: Add failing store tests with a mocked Tauri invoke**

```ts
import { beforeEach, describe, expect, test, vi } from "vitest";
import { createChatStore } from "./chat";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

beforeEach(() => invoke.mockReset());

function prompt(askId: string) {
  return {
    turn_id:"t1", round_num:0, ask_id:askId, mode:"single" as const,
    questions:[{ id:"q1", question:"Pick", options:["A"], allow_custom:false }],
  };
}

test("keeps the active ask until agent acknowledgement", async () => {
  invoke.mockResolvedValue(undefined);
  const chat = createChatStore("seed-a");
  chat.handleAskUser({ turn_id:"t1", round_num:0, ask_id:"a1", mode:"single", questions:[
    { id:"q1", question:"Pick", options:["A"], allow_custom:false }
  ]});
  await chat.submitAskAnswer([{ question_id:"q1", answer:"A" }]);
  expect(chat.askState().askId).toBe("a1");
  expect(chat.askState().submitting).toBe(true);
  chat.handleAskResolved("a1", "answered");
  expect(chat.askState().show).toBe(false);
});

test("queues distinct asks and deduplicates replayed ids", () => {
  const chat = createChatStore("seed-a");
  chat.handleAskUser(prompt("a1"));
  chat.handleAskUser(prompt("a1"));
  chat.handleAskUser(prompt("a2"));
  expect(chat.askQueue().map(item => item.askId)).toEqual(["a1", "a2"]);
});

test("transport rejection preserves the form and exposes the error", async () => {
  invoke.mockRejectedValue(new Error("pipe closed"));
  const chat = createChatStore("seed-a");
  chat.handleAskUser(prompt("a1"));
  await chat.submitAskAnswer([{ question_id:"q1", answer:"A" }]);
  expect(chat.askState().askId).toBe("a1");
  expect(chat.askState().submitting).toBe(false);
  expect(chat.askState().error).toContain("pipe closed");
});
```

Add these companion store tests:

```ts
test("ChatStores do not share prompt identity or cleanup", () => {
  const first = createChatStore("seed-a");
  const second = createChatStore("seed-b");
  first.handleAskUser(prompt("a1"));
  second.handleAskUser(prompt("b1"));
  first.handleAskResolved("a1", "answered");
  expect(first.askState().show).toBe(false);
  expect(second.askState().askId).toBe("b1");
});

test("cancel clears only the affected store queue", () => {
  const first = createChatStore("seed-a");
  const second = createChatStore("seed-b");
  first.handleAskUser(prompt("a1"));
  second.handleAskUser(prompt("b1"));
  first.handleCancelled();
  expect(first.askQueue()).toEqual([]);
  expect(second.askQueue().map(item => item.askId)).toEqual(["b1"]);
});

test("session replacement clears stale asks", () => {
  const chat = createChatStore("seed-a");
  chat.handleAskUser(prompt("a1"));
  chat.handleSessionCreated("seed-new");
  expect(chat.askQueue()).toEqual([]);
});
```

- [ ] **Step 3: Run frontend tests and verify RED**

Run: `pnpm --dir crates/deepx-tauri test`

Expected: current `askLock` drops the second ID and the form disappears before acknowledgement.

- [ ] **Step 4: Implement the acknowledged queue**

Use this store shape:

```ts
export interface AskState {
  askId: string;
  turnId: string;
  roundNum: number;
  mode: AskMode;
  questions: AskQuestion[];
  submitting: boolean;
  error?: string;
  show: boolean;
}

const EMPTY_ASK: AskState = {
  askId:"", turnId:"", roundNum:0, mode:"single", questions:[], submitting:false, show:false,
};
const [askQueue, setAskQueue] = createSignal<AskState[]>([]);
const askState = createMemo(() => askQueue()[0] ?? EMPTY_ASK);
```

`handleAskUser` rejects empty IDs, deduplicates an existing ID, and appends distinct IDs. `submitAskAnswer` marks only the front item submitting and invokes `cmd_ask_response` without dequeuing. `handleAskResolved` removes the matching ID. `handleAskRejected` retains it, clears `submitting`, and writes the backend message. `dismissAsk` invokes `cmd_ask_dismiss` and waits for `AskResolved::Dismissed` before removal. `clearAskQueue` is called from cancel, session replacement, undo of the active turn, and agent-death cleanup.

Delete all ask parsing from `handleToolResults`; direct `ask_user` is the only prompt entry.

- [ ] **Step 5: Route protocol events without synthetic identity**

In `App.tsx`:

```ts
case "ask_user": chat.handleAskUser({
  turn_id: p.turn_id as string,
  round_num: p.round_num as number,
  ask_id: p.ask_id as string,
  mode: p.mode as AskMode,
  questions: p.questions as AskQuestion[],
}); break;
case "ask_resolved": chat.handleAskResolved(p.ask_id as string, p.resolution as AskResolution); break;
case "ask_rejected": chat.handleAskRejected(p.ask_id as string, p.message as string); break;
case "done": chat.handleDone(); break;
```

Remove the `|| "0"` fallback and the duplicate `handleDone()` call. Keep routing to the `chat` argument supplied for `listenerSeed`, not `activeChat()`.

- [ ] **Step 6: Keep the Tauri commands typed and transport-only**

```rust
#[tauri::command]
pub fn cmd_ask_response(seed: String, ask_id: String, answers: Vec<deepx_proto::AskAnswer>) -> Result<(), String> {
    ensure_agent(&seed)?;
    send_to_agent(&seed, Ui2Agent::AskResponse { ask_id, answers })
}

#[tauri::command]
pub fn cmd_ask_dismiss(seed: String, ask_id: String) -> Result<(), String> {
    ensure_agent(&seed)?;
    send_to_agent(&seed, Ui2Agent::AskDismiss { ask_id })
}
```

Register both commands once. Do not treat the Tauri invocation result as an agent acknowledgement.

- [ ] **Step 7: Verify store tests, typecheck/build, and Tauri check**

```powershell
pnpm --dir crates/deepx-tauri test
pnpm --dir crates/deepx-tauri build
cargo check -p deepx-tauri
```

Expected: store tests pass, TypeScript has no unsafe event-shape errors in the ask path, and the Rust desktop bridge compiles.

- [ ] **Step 8: Commit the transport/store slice**

```powershell
git add -- crates/deepx-tauri/src-tauri/src/agent_bridge.rs crates/deepx-tauri/src-tauri/src/lib.rs crates/deepx-tauri/src/App.tsx crates/deepx-tauri/src/store/chat.ts crates/deepx-tauri/src/components/ToolRow.tsx crates/deepx-tauri/package.json crates/deepx-tauri/pnpm-lock.yaml crates/deepx-tauri/vite.config.ts crates/deepx-tauri/src/store/chat.ask.test.ts
git commit -m "fix(tauri): acknowledge and queue ask prompts per session"
```

---

### Task 7: Single and batch form correctness

**Files:**
- Modify: `crates/deepx-tauri/src/components/AskDialog.tsx`
- Create or Modify: `crates/deepx-tauri/src/components/AskForm.tsx`
- Modify: `crates/deepx-tauri/src/components/ChatView.tsx`
- Create: `crates/deepx-tauri/src/components/AskForm.test.tsx`
- Create: `crates/deepx-tauri/src/components/AskDialog.test.tsx`

**Interfaces:**
- Consumes: front `AskState` plus store submit/dismiss callbacks.
- Produces: exact visible answers, complete batch submissions, reset drafts by `askId`, and disabled controls during submission.

- [ ] **Step 1: Add failing DOM tests**

Render with Solid's DOM renderer into a fresh element and dispose after each test. Define the fixture in the test file:

```tsx
import { render } from "solid-js/web";
import { createSignal } from "solid-js";
import type { AskState } from "../store/chat";

function batchState(askId: string, submitting = false): AskState {
  return {
    askId, turnId:"t1", roundNum:0, mode:"batch", submitting, show:true,
    questions:[
      { id:"q1", question:"First?", options:[], allow_custom:true },
      { id:"q2", question:"Second?", options:["C","D"], allow_custom:false },
    ],
  };
}

test("custom batch text is submitted without pressing Enter", () => {
  const submitted = vi.fn();
  const state = () => batchState("ask-1");
  const root = document.createElement("div");
  const dispose = render(() => <AskForm state={state} onSubmit={submitted} onDismiss={() => {}} />, root);
  const inputs = root.querySelectorAll<HTMLInputElement>("input");
  inputs[0].value = "custom first";
  inputs[0].dispatchEvent(new InputEvent("input", { bubbles:true, data:"custom first" }));
  root.querySelectorAll<HTMLButtonElement>(".ask-option-btn")[1].click();
  root.querySelector<HTMLButtonElement>(".ask-submit-btn")!.click();
  expect(submitted).toHaveBeenCalledWith([
    { question_id:"q1", answer:"custom first" },
    { question_id:"q2", answer:"D" },
  ]);
  dispose();
});

test("incomplete batch cannot submit", () => {
  const submitted = vi.fn();
  const state = () => batchState("ask-1");
  const root = document.createElement("div");
  const dispose = render(() => <AskForm state={state} onSubmit={submitted} onDismiss={() => {}} />, root);
  root.querySelector<HTMLButtonElement>(".ask-option-btn")!.click();
  expect(root.querySelector<HTMLButtonElement>(".ask-submit-btn")!.disabled).toBe(true);
  expect(submitted).not.toHaveBeenCalled();
  dispose();
});
```

Add the remaining identity and submission assertions with these test bodies:

```tsx
test("changing askId clears local drafts", () => {
  const [state, setState] = createSignal(batchState("ask-1"));
  const submitted = vi.fn();
  const root = document.createElement("div");
  const dispose = render(() => <AskForm state={state} onSubmit={submitted} onDismiss={() => {}} />, root);
  const input = root.querySelector<HTMLInputElement>("input")!;
  input.value = "old draft";
  input.dispatchEvent(new InputEvent("input", { bubbles:true, data:"old draft" }));
  setState(batchState("ask-2"));
  expect(root.querySelector<HTMLInputElement>("input")!.value).toBe("");
  dispose();
});

test("submitting state disables batch controls", () => {
  const root = document.createElement("div");
  const dispose = render(() => <AskForm state={() => batchState("ask-1", true)} onSubmit={() => {}} onDismiss={() => {}} />, root);
  expect(root.querySelector<HTMLButtonElement>(".ask-submit-btn")!.disabled).toBe(true);
  expect(root.querySelector<HTMLButtonElement>(".ask-close")!.disabled).toBe(true);
  dispose();
});

test("single option sends the active question identity once", () => {
  const submitted = vi.fn();
  const state = () => ({
    askId:"single-1", turnId:"t1", roundNum:0, mode:"single" as const,
    questions:[{ id:"choice", question:"Pick", options:["A"], allow_custom:false }],
    submitting:false, show:true,
  });
  const root = document.createElement("div");
  const dispose = render(() => <AskDialog state={state} onSubmit={submitted} onDismiss={() => {}} />, root);
  root.querySelector<HTMLButtonElement>(".ask-option-btn")!.click();
  expect(submitted).toHaveBeenCalledTimes(1);
  expect(submitted).toHaveBeenCalledWith([{ question_id:"choice", answer:"A" }]);
  dispose();
});
```

- [ ] **Step 2: Run component tests and verify RED**

Run: `pnpm --dir crates/deepx-tauri test -- AskForm AskDialog`

Expected: current custom input is not committed until Enter, incomplete batches still invoke submit, and local drafts survive an ID change.

- [ ] **Step 3: Implement one source of truth for visible answers**

In `AskForm`, keep only one `Record<string,string>` for answers. Every input event writes directly to that record, and every option click replaces the same question entry. Reset it on identity change:

```ts
createEffect(on(() => props.state().askId, () => setAnswers({})));
const allAnswered = () => props.state().questions.every(q => (answers()[q.id] ?? "").trim().length > 0);
const result = () => props.state().questions.map(q => ({ question_id:q.id, answer:(answers()[q.id] ?? "").trim() }));
```

Set the submit button's `disabled` property to `!allAnswered() || s().submitting`. Do not call `onSubmit` when incomplete. Show `s().error` inside the dialog and keep the form mounted until acknowledgement.

In `AskDialog`, reset custom text when `askId` changes, block option/custom submission while `submitting`, and emit the active question ID exactly once.

- [ ] **Step 4: Run component and production frontend gates**

```powershell
pnpm --dir crates/deepx-tauri test
pnpm --dir crates/deepx-tauri build
```

Expected: all store/component tests pass and the production bundle builds.

- [ ] **Step 5: Commit the form slice**

```powershell
git add -- crates/deepx-tauri/src/components/AskDialog.tsx crates/deepx-tauri/src/components/AskForm.tsx crates/deepx-tauri/src/components/ChatView.tsx crates/deepx-tauri/src/components/AskForm.test.tsx crates/deepx-tauri/src/components/AskDialog.test.tsx
git commit -m "fix(ui): submit complete ask forms with stable identity"
```

---

### Task 8: Production-path verification and final refactor report

**Files:**
- Modify only if verification exposes a direct defect: files already listed in Tasks 1-7.
- Create: `docs/superpowers/reports/2026-07-14-ask-user-verification.md`

**Interfaces:**
- Consumes: completed implementation and real `deepx-tauri.exe --agent` JSON-LP behavior.
- Produces: requirement-by-requirement evidence and separately classified follow-up recommendations.

- [ ] **Step 1: Run focused Rust and frontend verification from a clean test process**

```powershell
cargo test -p deepx-tools ask_user::tests -- --nocapture
cargo test -p deepx-proto agent_protocol::tests -- --nocapture
cargo test -p deepx-msglp --test ask_user_lifecycle -- --test-threads=1 --nocapture
cargo test -p deepx-msglp --test permission_lifecycle -- --test-threads=1 --nocapture
cargo check -p deepx-msglp
cargo check -p deepx-tauri
pnpm --dir crates/deepx-tauri test
pnpm --dir crates/deepx-tauri build
```

Expected: every command exits 0; no lifecycle test relies on `Ui2Agent::ToolCall` as proof of the model path.

- [ ] **Step 2: Build and smoke the actual desktop agent entry point**

Run: `cargo build -p deepx-tauri`

Launch `target/debug/deepx-tauri.exe --agent` against a local sequential mock SSE server and send JSON-LP `CreateSession`, `UserInput`, partial/complete `AskResponse`, two queued ask responses, `AskDismiss`, and a fresh `UserInput`. Capture and assert:

- direct `Agent2Ui::AskUser` contains original call ID, turn ID, round number, mode, and all questions;
- the request counter stays unchanged after a partial or first queued answer;
- `AskRejected` leaves the active ask pending;
- `AskResolved` precedes the next ask or resumed model request;
- model request bodies contain exact accepted answer strings under the correct call IDs;
- dismissal does not swallow the next input;
- one `TurnEnd` and one `Done` appear per completed/aborted turn.

- [ ] **Step 3: Check diff hygiene and scope**

```powershell
git diff --check
git diff --stat HEAD~8..HEAD
git status --short
```

Expected: no unrelated crate formatting churn; untracked `SkillInfo.ts` or `SkillsStatus.ts` files are included only if generated binding verification proves they belong to this protocol regeneration, otherwise they remain outside ask commits.

- [ ] **Step 4: Write the evidence report**

The report must contain a compact table with each original question: multi-question first-answer race, frontend design, production new Ring hookup, blocking/wrong-answer behavior, and other races. Each row names the test or smoke artifact that proves the conclusion. Add four separate recommendation sections:

1. other-crate refactoring worth scheduling;
2. `ask_user` tool-only hardening;
3. new Ring hardening;
4. legacy Loop removal or migration.

Recommendations are not marked fixed unless a listed command or runtime trace proves them.

- [ ] **Step 5: Apply verification-before-completion and commit the report**

Read and follow `superpowers:verification-before-completion`, rerun any command whose output is stale after the final edit, then:

```powershell
git add -- docs/superpowers/reports/2026-07-14-ask-user-verification.md
git commit -m "docs: verify ask_user production lifecycle"
```
