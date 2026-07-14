# DeepX Tauri Protocol and Presentation Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish a compiling baseline, typed permission risk, exhaustive Agent2Ui reduction, ordered exec progress, and pure turn projection before replacing visual components.

**Architecture:** Rust remains authoritative for permission risk and wire facts. The Solid frontend receives generated DTOs into a raw session reducer, then pure projection functions produce UI-facing turns without importing components.

**Tech Stack:** Rust 2024, ts-rs, Tauri 2, SolidJS store, TypeScript 6, Vitest 4.

## Global Constraints

- Preserve all existing protocol variants unless a producer search proves a variant obsolete.
- `PermissionLevel` is policy, not action risk; never derive button color from `level`.
- Use stable `tool_call_id` and ordered `seq` for exec output.
- Do not replace conversation components in this plan.
- Stage and commit only files listed by each task.

---

### Task 1: Restore the frontend and permission-command baseline

**Files:**
- Modify: `crates/deepx-tauri/src/components/SettingsView.tsx:302`
- Modify: `crates/deepx-tauri/src-tauri/src/lib.rs:15`
- Test: `crates/deepx-tauri/src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: existing `agent_bridge::cmd_permission_response`.
- Produces: a compiling JSX tree and a registered Tauri permission command.

- [ ] **Step 1: Record the failing frontend build**

Run:

```powershell
pnpm --dir crates/deepx-tauri run build
```

Expected: FAIL at `SettingsView.tsx` with an unclosed `section` error.

- [ ] **Step 2: Close the API-key section before the Subagent section**

Change the boundary to:

```tsx
            <div class="settings-row">
            </div>
          </section>

          {/* ── Subagent ── */}
          <section class="settings-card">
```

- [ ] **Step 3: Add the missing handler registration and reachability assertion**

Add `agent_bridge::cmd_permission_response` beside the other permission commands:

```rust
.invoke_handler(tauri::generate_handler![
    agent_bridge::cmd_send_message,
    agent_bridge::cmd_permission_response,
    agent_bridge::cmd_ask_response,
    agent_bridge::cmd_ask_dismiss,
    // existing handlers remain in their current order
])
```

Add to the existing reachability test module:

```rust
#[test]
fn permission_response_command_is_reachable() {
    let _ = agent_bridge::cmd_permission_response as fn(String, String, bool, bool) -> _;
}
```

- [ ] **Step 4: Verify baseline checks**

Run:

```powershell
pnpm --dir crates/deepx-tauri run build
cargo test -p deepx-tauri permission_response_command_is_reachable
```

Expected: both commands PASS.

- [ ] **Step 5: Commit the baseline repair**

```powershell
git add crates/deepx-tauri/src/components/SettingsView.tsx crates/deepx-tauri/src-tauri/src/lib.rs
git commit -m "fix(tauri): restore frontend and permission command baseline"
```

### Task 2: Add backend-owned permission risk

**Files:**
- Modify: `crates/deepx-tools/src/permission.rs`
- Modify: `crates/deepx-tools/src/authorization.rs`
- Test: `crates/deepx-tools/src/permission.rs`
- Test: `crates/deepx-tools/src/authorization.rs`

**Interfaces:**
- Consumes: `ToolCategory`, normalized paths, and workspace root.
- Produces: `PermissionRisk`, `PermissionDecision::AskUser { risk, consequence, ... }`, and `PermissionChallenge::risk()/consequence()`.

- [ ] **Step 1: Write risk-classification tests**

Add tests covering the approved mapping:

```rust
#[test]
fn permission_risk_distinguishes_read_workspace_write_and_exec() {
    let ws = PathBuf::from("C:/repo");
    assert_eq!(classify_risk(ToolCategory::Read, &[], &ws), PermissionRisk::Low);
    assert_eq!(
        classify_risk(ToolCategory::Write, &[ws.join("src/lib.rs")], &ws),
        PermissionRisk::Medium
    );
    assert_eq!(classify_risk(ToolCategory::Exec, &[], &ws), PermissionRisk::High);
    assert_eq!(
        classify_risk(ToolCategory::Write, &[PathBuf::from("C:/outside/file")], &ws),
        PermissionRisk::High
    );
}
```

- [ ] **Step 2: Run the targeted test to verify it fails**

Run:

```powershell
cargo test -p deepx-tools permission_risk_distinguishes_read_workspace_write_and_exec
```

Expected: FAIL because `PermissionRisk` and `classify_risk` do not exist.

- [ ] **Step 3: Implement risk and consequence as decision facts**

Add:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionRisk { Low, Medium, High }

pub fn classify_risk(category: ToolCategory, paths: &[PathBuf], workspace: &Path) -> PermissionRisk {
    if matches!(category, ToolCategory::Exec | ToolCategory::Net) {
        return PermissionRisk::High;
    }
    if paths.iter().any(|path| !resolve_target_path(path.clone()).starts_with(workspace)) {
        return PermissionRisk::High;
    }
    match category {
        ToolCategory::Read => PermissionRisk::Low,
        ToolCategory::Write => PermissionRisk::Medium,
        ToolCategory::Exec | ToolCategory::Net => PermissionRisk::High,
    }
}
```

Extend `PermissionDecision::AskUser` and `PermissionChallenge` with `risk` and `consequence`. Set consequence deterministically:

```rust
let consequence = match risk {
    PermissionRisk::Low => "Reads data without changing it.",
    PermissionRisk::Medium => "Changes files inside the current workspace.",
    PermissionRisk::High => "May affect external resources or execute arbitrary actions.",
}.to_string();
```

- [ ] **Step 4: Verify permission and authorization tests**

Run:

```powershell
cargo test -p deepx-tools permission
cargo test -p deepx-tools authorization
```

Expected: PASS with explicit low/medium/high cases.

- [ ] **Step 5: Commit backend risk classification**

```powershell
git add crates/deepx-tools/src/permission.rs crates/deepx-tools/src/authorization.rs
git commit -m "feat(tools): classify permission action risk"
```

### Task 3: Carry risk and final-round facts through the protocol

**Files:**
- Modify: `crates/deepx-proto/src/agent_protocol.rs:198`
- Modify: `crates/deepx-msglp/src/new/engine_tool.rs:69`
- Modify: `crates/deepx-msglp/src/util.rs:218`
- Modify: `crates/deepx-msglp/tests/permission_lifecycle.rs`
- Generated: `crates/deepx-tauri/src/lib/types/Agent2Ui.ts`
- Generated: `crates/deepx-tauri/src/lib/types/RoundData.ts`
- Create: `crates/deepx-tauri/src/lib/types/PermissionRisk.ts`

**Interfaces:**
- Consumes: `deepx_tools::permission::PermissionRisk` and existing `Agent2Ui::RoundComplete.is_final`.
- Produces: serialized `risk`, `consequence`, and restored `RoundData.is_final`.

- [ ] **Step 1: Extend the permission lifecycle assertion**

Match the emitted request:

```rust
match expect_event(receiver, Duration::from_secs(5), |event| {
    matches!(event, Agent2Ui::PermissionRequest { .. })
}) {
    Agent2Ui::PermissionRequest { risk, consequence, .. } => {
        assert_eq!(risk, PermissionRisk::High);
        assert!(!consequence.is_empty());
    }
    _ => unreachable!(),
}
```

- [ ] **Step 2: Run the lifecycle test to verify it fails**

Run:

```powershell
cargo test -p deepx-msglp --test permission_lifecycle
```

Expected: compile FAIL because protocol risk fields are absent.

- [ ] **Step 3: Add protocol DTOs and producer mapping**

In `deepx-proto` add:

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PermissionRisk { Low, Medium, High }
```

Extend `RoundData`:

```rust
#[serde(default)]
pub is_final: bool,
```

Extend `Agent2Ui::PermissionRequest`:

```rust
risk: PermissionRisk,
consequence: String,
```

Map the tool-layer enum in both `PermissionRequest` emission sites, and set restored rounds in `util.rs` with `is_final: round_index + 1 == round_count`.

- [ ] **Step 4: Regenerate TypeScript and run protocol/lifecycle checks**

Run:

```powershell
cargo test -p deepx-proto
cargo test -p deepx-msglp --test permission_lifecycle
cargo build -p deepx-tauri
pnpm --dir crates/deepx-tauri run build
```

Expected: PASS; generated TypeScript contains `PermissionRisk`, `risk`, `consequence`, and `RoundData.is_final`.

- [ ] **Step 5: Commit the wire contract**

```powershell
git add crates/deepx-proto/src/agent_protocol.rs crates/deepx-msglp/src/new/engine_tool.rs crates/deepx-msglp/src/util.rs crates/deepx-msglp/tests/permission_lifecycle.rs crates/deepx-tauri/src/lib/types
git commit -m "feat(proto): expose permission risk and final rounds"
```

### Task 4: Introduce raw session state and exhaustive event reduction

**Files:**
- Create: `crates/deepx-tauri/src/store/rawSession.ts`
- Create: `crates/deepx-tauri/src/store/sessionEventReducer.ts`
- Create: `crates/deepx-tauri/src/store/sessionEventReducer.test.ts`
- Modify: `crates/deepx-tauri/src/App.tsx:124`

**Interfaces:**
- Consumes: generated `Agent2Ui`, `TurnData`, `RoundBlock`, `ToolCallDef`, and `ToolResultDef`.
- Produces: `createRawSessionState(seed)`, `reduceAgentEvent(state, event, now)`, and `assertNever(value)`.

- [ ] **Step 1: Write reducer tests for final rounds and missing variants**

```ts
import { createRawSessionState, reduceAgentEvent } from "./sessionEventReducer";

it("retains final-round and code-delta facts", () => {
  let state = createRawSessionState("seed-a");
  state = reduceAgentEvent(state, { type: "turn_start", turn_id: "t1", user_text: "go" }, 100);
  state = reduceAgentEvent(state, {
    type: "round_complete", turn_id: "t1", round_num: 1,
    answer: "done", tool_calls: [], blocks: [{ type: "text", content: "done" }], is_final: true,
  }, 200);
  state = reduceAgentEvent(state, {
    type: "code_delta", lines_added: 7, lines_removed: 2,
    files_created: 0, files_deleted: 0, file: "src/App.tsx",
  }, 210);
  expect(state.turns[0].rounds[0].isFinal).toBe(true);
  expect(state.environment.linesAdded).toBe(7);
});
```

- [ ] **Step 2: Run the test to verify it fails**

```powershell
pnpm --dir crates/deepx-tauri exec vitest run src/store/sessionEventReducer.test.ts
```

Expected: FAIL because reducer files do not exist.

- [ ] **Step 3: Implement raw types and exhaustive reduction**

Define focused facts:

```ts
export type RawRound = {
  roundNum: number;
  isFinal: boolean;
  blocks: RoundBlock[];
  toolCalls: ToolCallDef[];
  toolResults: Record<string, ToolResultDef>;
  progress: Record<string, OrderedProgress>;
};

export function assertNever(value: never): never {
  throw new Error(`Unhandled Agent2Ui event: ${JSON.stringify(value)}`);
}
```

Implement `reduceAgentEvent` as `switch (event.type)` with a case for every generated variant. Lifecycle-only variants return the unchanged state explicitly. During the comparison period, `App.tsx` feeds every event into both the new raw reducer and the existing legacy chat handlers; this preserves the old renderer until the integration plan deletes it. Tauri/listener side effects remain in `App`, while new state facts are owned only by the reducer.

- [ ] **Step 4: Run reducer and frontend tests**

```powershell
pnpm --dir crates/deepx-tauri exec vitest run src/store/sessionEventReducer.test.ts
pnpm --dir crates/deepx-tauri run build
```

Expected: PASS; TypeScript accepts the exhaustive switch.

- [ ] **Step 5: Commit the reducer boundary**

```powershell
git add crates/deepx-tauri/src/store/rawSession.ts crates/deepx-tauri/src/store/sessionEventReducer.ts crates/deepx-tauri/src/store/sessionEventReducer.test.ts crates/deepx-tauri/src/App.tsx
git commit -m "refactor(tauri): add exhaustive raw session reducer"
```

### Task 5: Add ordered exec buffering and pure turn projection

**Files:**
- Create: `crates/deepx-tauri/src/store/orderedProgress.ts`
- Create: `crates/deepx-tauri/src/store/orderedProgress.test.ts`
- Create: `crates/deepx-tauri/src/presentation/turnProjection.ts`
- Create: `crates/deepx-tauri/src/presentation/turnProjection.test.ts`
- Create: `crates/deepx-tauri/src/presentation/processAggregation.ts`
- Create: `crates/deepx-tauri/src/presentation/processAggregation.test.ts`

**Interfaces:**
- Consumes: `RawTurn` and `OrderedProgress`.
- Produces: `emptyProgress()`, `appendProgress(buffer, event)`, `materializeProgress(buffer)`, `projectTurn(rawTurn)`, `aggregateProcessItems(items)`.

- [ ] **Step 1: Write ordering and projection tests**

```ts
it("orders stdout and stderr by seq without losing provenance", () => {
  let b = emptyProgress();
  b = appendProgress(b, { stream: "stdout", seq: 2, chunk: "B" });
  b = appendProgress(b, { stream: "stderr", seq: 1, chunk: "E" });
  expect(materializeProgress(b)).toEqual([
    { stream: "stderr", seq: 1, chunk: "E" },
    { stream: "stdout", seq: 2, chunk: "B" },
  ]);
});

it("projects only the final round answer at top level", () => {
  const view = projectTurn(rawTurnWithIntermediateAndFinalAnswers());
  expect(view.process.items.some(i => i.kind === "assistant_progress")).toBe(true);
  expect(view.finalAnswer?.markdown).toBe("final answer");
});
```

- [ ] **Step 2: Run tests to verify they fail**

```powershell
pnpm --dir crates/deepx-tauri exec vitest run src/store/orderedProgress.test.ts src/presentation/turnProjection.test.ts src/presentation/processAggregation.test.ts
```

Expected: FAIL because the modules do not exist.

- [ ] **Step 3: Implement the pure interfaces**

Use these stable view types:

```ts
export type ProcessItem =
  | { kind: "reasoning"; id: string; content: string; elapsedMs?: number }
  | { kind: "assistant_progress"; id: string; markdown: string }
  | { kind: "tool"; id: string; toolName: string; summary: string; success?: boolean }
  | { kind: "group"; id: string; family: string; label: string; children: ProcessItem[] }
  | { kind: "interaction"; id: string; label: string; resolution: string }
  | { kind: "notice"; id: string; level: string; message: string };

export type TurnViewModel = {
  turnId: string;
  userPrompt: string;
  process: { status: RawTurn["status"]; elapsedMs?: number; items: ProcessItem[] };
  finalAnswer?: { markdown: string };
};
```

Implement ordered progress with the exact public surface used by the tests:

```ts
export const emptyProgress = (): OrderedProgress => ({ chunks: new Map(), nextExpectedSeq: 0 });

export function appendProgress(buffer: OrderedProgress, event: ProgressEvent): OrderedProgress {
  const chunks = new Map(buffer.chunks);
  chunks.set(event.seq, event);
  return { chunks, nextExpectedSeq: buffer.nextExpectedSeq };
}

export function materializeProgress(buffer: OrderedProgress): ProgressEvent[] {
  return [...buffer.chunks.values()].sort((a, b) => a.seq - b.seq);
}
```

Aggregation groups consecutive successful operations only. A failed child ends the current group and remains a top-level process item.

- [ ] **Step 4: Run the foundation gate**

```powershell
pnpm --dir crates/deepx-tauri run test:run
pnpm --dir crates/deepx-tauri run build
cargo check -p deepx-tauri
```

Expected: PASS with no visual component replacement yet.

- [ ] **Step 5: Commit the presentation foundation**

```powershell
git add crates/deepx-tauri/src/store/orderedProgress.ts crates/deepx-tauri/src/store/orderedProgress.test.ts crates/deepx-tauri/src/presentation
git commit -m "feat(tauri): project raw turns into process transcripts"
```
