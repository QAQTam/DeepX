# DeepX Tauri Legacy Frontend Removal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Completely delete the legacy DeepX Tauri frontend implementation while preserving every active legacy data-flow responsibility in one typed, reload-stable `RawSessionState` pipeline.

**Architecture:** Generated `Agent2Ui` events enter through one runtime-checked boundary, pass through a typed replay coordinator, and reduce into one `RawSessionState` per session. A focused session registry owns the raw signal, batched/persisted runtime, listener, and a tiny local UI state; pure selectors drive `ChatView`, while `App.tsx` owns only Tauri commands and non-state side effects. The existing Tauri lifecycle replay cache and `sessionStorage` snapshot remain the Phase 0 refresh bridge; no backend execution architecture changes are permitted.

**Tech Stack:** SolidJS 1.9, TypeScript 6, Vitest 4/jsdom, Tauri 2 JavaScript APIs, generated Rust `ts-rs` protocol bindings, pnpm, PowerShell.

## Global Constraints

- Scope is `crates/deepx-tauri` frontend architecture cleanup only.
- Do not introduce `deepx.exe`, WebSocket, WinUI 3, ratatui, protocol V2, a backend thread manager, or a broad Ring-loop/`deepx-msglp` refactor.
- Do not hand-edit generated TypeScript bindings under `src/lib/types`; this plan consumes the current generated `Agent2Ui` union.
- `RawSessionState` is the only event-derived session state. Local UI state must not contain turns, rounds, streaming, pagination, usage, skills, compact payloads, pending interaction payloads, or restore data.
- Register listeners before resume/start, replay backend lifecycle events before buffered live events, persist every committed raw state, and never stop a backend session from WebView cleanup.
- Keep keyed reactive conversation rows, latest-wins Markdown rendering, animation-frame delta batching, reload snapshots, and lifecycle replay behavior.
- Add no runtime dependency and do not redesign visual appearance beyond mounting the existing `AppShell`/`TaskSidebar` and removing the hidden legacy shell.
- Tests must be written and observed failing before each production change.
- The user-owned `crates/deepx-tauri/src-tauri/Cargo.toml` must remain unstaged and byte-identical. Its working-file hash at plan creation is `5b80caae02abced664f6801fbd98fb512e3d979e`.
- Stage explicit paths only. Do not run workspace-wide formatting.

---

## Baseline Evidence

- Plan baseline: `main@1cb41cd`.
- Current automated baseline: 31 frontend test files and 100 tests pass; the frontend build, protocol binding tests, and `cargo check -p deepx-tauri --tests` pass.
- `App.tsx` still creates `chatStores`, `rawSessions`, `rawEventRuntimes`, `permissionQueue`, and a hidden legacy sidebar.
- `ChatView` renders transcript rows from `RawSessionState` but reads composer, compact, interactions, workspace, tasks, metrics, model, and context values from `createChatStore`.
- `sessionReplayBuffer` and Tauri listeners still accept `Record<string, unknown>` instead of generated `Agent2Ui`.
- `AppShell` and `TaskSidebar` exist and are tested, but `AppShell` is not mounted.

## File Structure

- Modify `crates/deepx-tauri/src/store/rawSession.ts` — complete event-derived state schema.
- Modify `crates/deepx-tauri/src/store/sessionEventReducer.ts` and `.test.ts` — exhaustive, idempotent event mapping and raw-state mutation helpers.
- Create `crates/deepx-tauri/src/store/sessionSelectors.ts` and `.test.ts` — streaming, interaction, usage, retry, and pagination selectors.
- Create `crates/deepx-tauri/src/store/sessionUiState.ts` and `.test.ts` — workspace and duplicate-submit guard only.
- Create `crates/deepx-tauri/src/store/sessionRegistry.ts` and `.test.ts` — per-session state/runtime/listener ownership and atomic seed remapping.
- Create `crates/deepx-tauri/src/runtime/agentEventBoundary.ts` and `.test.ts` — runtime validation of the generated event discriminant.
- Modify `crates/deepx-tauri/src/runtime/sessionReplayBuffer.ts` and `.test.ts` — typed replay/live ordering.
- Modify `crates/deepx-tauri/src/store/sessionEventRuntime.ts` and `.test.ts` — reload schema v2 and safe persistence behavior.
- Create `crates/deepx-tauri/src/runtime/agentEventDispatcher.ts` and `.test.ts` — one exhaustive reducer/side-effect dispatcher.
- Modify `crates/deepx-tauri/src/components/PlanReviewPanel.tsx` and `.test.tsx` — make plan review presentation-only so one controller sends one command.
- Modify `crates/deepx-tauri/src/components/conversation/ConversationTranscript.tsx` and tests — real older-turn pagination.
- Modify `crates/deepx-tauri/src/components/ChatView.tsx` and interaction tests in the same atomic task as `App.tsx` — raw/selectors/local-UI-only rendering without an uncompilable intermediate commit.
- Modify `crates/deepx-tauri/src/components/shell/AppShell.tsx` and tests — exactly one application shell/sidebar.
- Modify `crates/deepx-tauri/src/App.tsx` — registry orchestration, typed listener/replay, commands, and side effects.
- Modify `crates/deepx-tauri/src/main.tsx` — remove legacy style imports.
- Create `crates/deepx-tauri/src/legacyFrontendRemoval.test.ts` — structural deletion guards.
- Delete all files listed in Task 8 after production imports are gone.

---

### Task 1: Complete the raw session model and event mapping

**Files:**
- Modify: `crates/deepx-tauri/src/store/rawSession.ts`
- Modify: `crates/deepx-tauri/src/store/sessionEventReducer.ts`
- Modify: `crates/deepx-tauri/src/store/sessionEventReducer.test.ts`
- Modify: `crates/deepx-tauri/src/store/sessionEventReducer.test.ts`

**Interfaces:**
- Consumes: generated `Agent2Ui`, `TaskInfo`, `AskQuestion`, `UsageInfo`, `PermissionRisk`, and restored `TurnData`.
- Produces: `RawSessionState.pendingInteractions`, `dashboard`, `telemetry`, complete compact state, `resolvePendingInteraction`, `applyDashboardData`, and `removeTurnFromSession`.

- [ ] **Step 1: Add failing reducer coverage for every legacy responsibility**

Append tests that exercise queueing, usage/dashboard mapping, compact completion, idempotency, terminal fallback, and undo:

```ts
it("queues interactions without overwriting an earlier gate", () => {
  let state = reduceAgentEvent(createRawSessionState("seed-a"), {
    type: "turn_start", turn_id: "t1", user_text: "run",
  }, 1);
  state = reduceAgentEvent(state, {
    type: "permission_request", tool_call_id: "perm-1", tool_name: "exec",
    reason: "run", paths: [], category: "exec", level: 4,
    risk: "medium", consequence: "runs a process",
  }, 2);
  state = reduceAgentEvent(state, {
    type: "ask_user", turn_id: "t1", round_num: 0, ask_id: "ask-1",
    mode: "single", questions: [{ id: "q1", question: "Continue?", options: [], allow_custom: true }],
  }, 3);

  expect(state.pendingInteractions.map(item => item.id)).toEqual(["perm-1", "ask-1"]);
  state = resolvePendingInteraction(state, "perm-1", "approved", 4);
  expect(state.pendingInteractions.map(item => item.id)).toEqual(["ask-1"]);
  expect(state.turns[0].status).toBe("waiting");
});

it("preserves streamed text and previews when round_complete omits optional fields", () => {
  let state = reduceAgentEvent(createRawSessionState("seed-a"), {
    type: "turn_start", turn_id: "t1", user_text: "run",
  }, 1);
  state = reduceAgentEvent(state, {
    type: "round_delta", turn_id: "t1", round_num: 0, kind: "thinking", delta: "think",
  }, 2);
  state = reduceAgentEvent(state, {
    type: "tool_call_preview", turn_id: "t1", round_num: 0, index: 0,
    id: "call-1", name: "exec", args_so_far: "{\"cmd\":\"dir\"}",
  }, 3);
  state = reduceAgentEvent(state, {
    type: "round_complete", turn_id: "t1", round_num: 0, is_final: false,
  }, 4);

  expect(state.turns[0].rounds[0].thinking).toBe("think");
  expect(state.turns[0].rounds[0].toolCalls[0].id).toBe("call-1");
});

it("maps usage, dashboard, audit, compact completion, done, and undo", () => {
  let state = reduceAgentEvent(createRawSessionState("seed-a"), {
    type: "turn_start", turn_id: "t1", user_text: "run",
  }, 10);
  state = reduceAgentEvent(state, {
    type: "turn_end", turn_id: "t1", usage: {
      prompt_tokens: 100, completion_tokens: 20, total_tokens: 120,
      prompt_cache_hit_tokens: 80, prompt_cache_miss_tokens: 20, reasoning_tokens: 5,
    },
  }, 20);
  state = reduceAgentEvent(state, {
    type: "dashboard", hp_connected: true, session_seed: "seed-a",
    tool_calls_total: 1, tool_failures: 0, current_phase: "done", streaming: false,
    dsml_compat_count: 0, tasks: [], recent_edits: ["src/a.ts"],
    session_title: "Title", context_limit: 200000, model: "model-a",
  }, 21);
  state = reduceAgentEvent(state, {
    type: "audit_record", tool_name: "exec", result_summary: "ok", success: true,
    time: "2026-07-16T00:00:00Z", args: "{}",
  }, 22);
  state = reduceAgentEvent(state, { type: "compact_start", turns_total: 4, turns_keeping: 2 }, 23);
  state = reduceAgentEvent(state, { type: "compact_delta", delta: "summary" }, 24);
  state = reduceAgentEvent(state, { type: "compact_end", summary_chars: 7, turns_compacted: 2 }, 25);

  expect(state.session.usage?.total_tokens).toBe(120);
  expect(state.dashboard.recentEdits).toEqual(["src/a.ts"]);
  expect(state.dashboard.activity[0].toolName).toBe("exec");
  expect(state.compact).toMatchObject({ active: false, turnsCompacted: 2 });
  expect(state.telemetry.at(-1)?.context_tokens).toBe(100);
  expect(removeTurnFromSession(state, "t1").turns).toHaveLength(0);
});

it("treats done as a terminal fallback and resets a newly created seed", () => {
  let state = reduceAgentEvent(createRawSessionState("old"), {
    type: "turn_start", turn_id: "t1", user_text: "run",
  }, 1);
  state = reduceAgentEvent(state, { type: "done" }, 2);
  expect(state.turns[0].status).toBe("completed");

  state = reduceAgentEvent(state, { type: "session_created", seed: "new" }, 3);
  expect(state.seed).toBe("new");
  expect(state.turns).toEqual([]);
  expect(state.session.ready).toBe(true);
});

it("is idempotent for replayed lifecycle and pagination events", () => {
  let state = reduceAgentEvent(createRawSessionState("seed-a"), {
    type: "session_created", seed: "seed-a",
  }, 1);
  state = reduceAgentEvent(state, { type: "turn_start", turn_id: "t1", user_text: "run" }, 2);
  state = reduceAgentEvent(state, { type: "turn_end", turn_id: "t1" }, 3);
  const completed = state;
  state = reduceAgentEvent(state, { type: "turn_end", turn_id: "t1" }, 4);
  expect(state).toBe(completed);

  const older = {
    turn_id: "older", user_text: "old", rounds: [],
  };
  state = reduceAgentEvent(state, { type: "more_turns", turns: [older], has_more: false }, 5);
  state = reduceAgentEvent(state, { type: "more_turns", turns: [older], has_more: false }, 6);
  expect(state.turns.filter(turn => turn.turnId === "older")).toHaveLength(1);
});
```

- [ ] **Step 2: Run the reducer tests and verify the new contract fails**

```powershell
cd F:\DeepX-Fork\crates\deepx-tauri
pnpm exec vitest run src/store/sessionEventReducer.test.ts
```

Expected: compile/test failures for missing `pendingInteractions`, dashboard/telemetry fields, compact result, and `removeTurnFromSession`; the optional round-complete test also exposes destructive replacement.

- [ ] **Step 3: Extend the raw state schema**

Replace the singular pending interaction and incomplete auxiliary fields with these exact shapes:

```ts
export type RawMetricPoint = {
  ts: number;
  context_tokens: number;
  cache_hit: number;
  cache_miss: number;
};

export type RawActivityEntry = {
  toolName: string;
  summary: string;
  success: boolean;
  time: string;
  args: string;
};

type InteractionBase = { id: string; turnId: string };

export type PendingInteraction =
  | (InteractionBase & {
      kind: "permission";
      toolName: string;
      reason: string;
      paths: string[];
      category: string;
      level: number;
      risk: PermissionRisk;
      consequence: string;
    })
  | (InteractionBase & {
      kind: "ask";
      roundNum: number;
      mode: AskMode;
      questions: AskQuestion[];
    })
  | (InteractionBase & { kind: "plan"; content: string });

export type DashboardData = {
  tasks: TaskInfo[];
  recentEdits: string[];
};
```

Add `TaskInfo` to the generated-type imports and change `RawSessionState` to contain:

```ts
/** Transitional read mirror removed in Task 7 after App and ChatView switch atomically. */
pendingInteraction: PendingInteraction | null;
pendingInteractions: PendingInteraction[];
dashboard: DashboardData & { activity: RawActivityEntry[] };
telemetry: RawMetricPoint[];
session: {
  ready: boolean;
  hasMore: boolean;
  totalTurns: number;
  tokensUsed: number;
  cacheHitPct: number;
  title?: string;
  model?: string;
  contextLimit: number;
  usage?: UsageInfo;
};
compact: {
  active: boolean;
  text: string;
  turnsCompacted: number | null;
  completionRevision: number;
};
```

- [ ] **Step 4: Implement idempotent interaction, telemetry, dashboard, terminal, and undo reducers**

Add these helpers and use them from the event cases:

```ts
const MAX_ACTIVITY = 50;
const MAX_METRICS = 120;

function appendMetric(state: RawSessionState, usage: UsageInfo, now: number): RawSessionState {
  return {
    ...state,
    telemetry: [...state.telemetry, {
      ts: now,
      context_tokens: usage.prompt_tokens,
      cache_hit: usage.prompt_cache_hit_tokens,
      cache_miss: usage.prompt_cache_miss_tokens,
    }].slice(-MAX_METRICS),
  };
}

function enqueueInteraction(state: RawSessionState, item: PendingInteraction): RawSessionState {
  if (state.pendingInteractions.some(current => current.kind === item.kind && current.id === item.id)) {
    return state;
  }
  const pendingInteractions = [...state.pendingInteractions, item];
  return { ...state, pendingInteractions, pendingInteraction: pendingInteractions[0] ?? null };
}

export function applyDashboardData(state: RawSessionState, data: DashboardData): RawSessionState {
  return { ...state, dashboard: { ...state.dashboard, ...data } };
}

export function removeTurnFromSession(state: RawSessionState, turnId: string): RawSessionState {
  const pendingInteractions = state.pendingInteractions.filter(item => item.turnId !== turnId);
  return {
    ...state,
    turns: state.turns.filter(turn => turn.turnId !== turnId),
    pendingInteractions,
    pendingInteraction: pendingInteractions[0] ?? null,
  };
}
```

Rewrite `resolvePendingInteraction` to remove by id, update the transitional `pendingInteraction` mirror from the new queue, record on the interaction's own turn, and leave that turn waiting while another gate for it remains. This mirror exists only to keep the current `App.tsx` and `ChatView.tsx` buildable through Tasks 1-6; Task 7 removes the field and all reads atomically. Update event cases with these rules:

```ts
export function resolvePendingInteraction(
  state: RawSessionState,
  id: string,
  resolution: string,
  now = Date.now(),
): RawSessionState {
  const interaction = state.pendingInteractions.find(item => item.id === id);
  if (!interaction) return state;
  const pendingInteractions = state.pendingInteractions.filter(item => item.id !== id);
  const next = {
    ...state,
    pendingInteractions,
    pendingInteraction: pendingInteractions[0] ?? null,
  };
  const stillWaiting = pendingInteractions.some(item => item.turnId === interaction.turnId);
  return updateTurn(next, interaction.turnId, turn => ({
    ...turn,
    status: stillWaiting ? "waiting" : turn.status === "waiting" ? "running" : turn.status,
    interactions: [...turn.interactions, {
      id, kind: interaction.kind, resolution, at: now,
    }],
  }));
}
```

```ts
case "round_complete":
  return updateRound(state, event.turn_id, event.round_num, round => ({
    ...round,
    isFinal: event.is_final,
    thinking: event.thinking ?? round.thinking,
    answer: event.answer ?? round.answer,
    toolCalls: event.tool_calls ?? round.toolCalls,
    blocks: event.blocks ?? round.blocks,
  }));

case "turn_end": {
  const current = state.turns.find(turn => turn.turnId === event.turn_id);
  if (
    current?.status === "completed" &&
    current.stopReason === event.stop_reason &&
    JSON.stringify(current.usage) === JSON.stringify(event.usage)
  ) return state;
  let next = updateTurn(state, event.turn_id, turn => ({
    ...turn,
    status: turn.status === "failed" || turn.status === "cancelled" ? turn.status : "completed",
    endedAt: now,
    stopReason: event.stop_reason,
    usage: event.usage,
  }));
  if (event.usage) {
    next = appendMetric({ ...next, session: { ...next.session, usage: event.usage } }, event.usage, now);
  }
  return next;
}

case "permission_request": {
  const turnId = lastTurnId(state);
  if (!turnId) return state;
  return updateTurn(enqueueInteraction(state, {
    kind: "permission", id: event.tool_call_id, turnId,
    toolName: event.tool_name, reason: event.reason, paths: event.paths,
    category: event.category, level: event.level, risk: event.risk,
    consequence: event.consequence,
  }), turnId, turn => ({ ...turn, status: "waiting" }));
}

case "ask_user":
  return updateTurn(enqueueInteraction(state, {
    kind: "ask", id: event.ask_id, turnId: event.turn_id,
    roundNum: event.round_num, mode: event.mode, questions: event.questions,
  }), event.turn_id, turn => ({ ...turn, status: "waiting" }));

case "plan_submitted": {
  const turnId = lastTurnId(state);
  if (!turnId) return state;
  return updateTurn(enqueueInteraction(state, {
    kind: "plan", id: event.call_id, turnId, content: event.plan_content,
  }), turnId, turn => ({ ...turn, status: "waiting" }));
}

case "compact_start":
  return { ...state, compact: { ...state.compact, active: true, text: "", turnsCompacted: null } };
case "compact_delta":
  return { ...state, compact: { ...state.compact, active: true, text: state.compact.text + event.delta } };
case "compact_end":
  if (!state.compact.active && state.compact.turnsCompacted === event.turns_compacted) return state;
  return { ...state, compact: {
    active: false, text: "", turnsCompacted: event.turns_compacted,
    completionRevision: state.compact.completionRevision + 1,
  } };
case "done": {
  const turnId = lastTurnId(state);
  return turnId ? updateTurn(state, turnId, turn =>
    turn.status === "running" || turn.status === "waiting"
      ? { ...turn, status: "completed", endedAt: now }
      : turn,
  ) : state;
}
case "session_created": {
  if (state.seed === event.seed && state.session.ready) return state;
  const created = createRawSessionState(event.seed);
  return { ...created, session: { ...created.session, ready: true } };
}
```

Use these exact dashboard and audit mappings:

```ts
case "dashboard": {
  let next: RawSessionState = {
    ...state,
    session: {
      ...state.session,
      title: event.session_title,
      model: event.model,
      contextLimit: event.context_limit,
      usage: event.usage ?? state.session.usage,
    },
    dashboard: {
      ...state.dashboard,
      tasks: event.tasks ?? state.dashboard.tasks,
      recentEdits: event.recent_edits ?? state.dashboard.recentEdits,
    },
  };
  if (event.usage) next = appendMetric(next, event.usage, now);
  return next;
}
case "audit_record": {
  const entry = {
    toolName: event.tool_name,
    summary: event.result_summary,
    success: event.success,
    time: event.time,
    args: event.args,
  };
  const previous = state.dashboard.activity[0];
  if (previous && JSON.stringify(previous) === JSON.stringify(entry)) return state;
  return {
    ...state,
    dashboard: {
      ...state.dashboard,
      activity: [entry, ...state.dashboard.activity].slice(0, MAX_ACTIVITY),
    },
  };
}
case "more_turns": {
  const existing = new Set(state.turns.map(turn => turn.turnId));
  const older = event.turns.map(restoredTurn).filter(turn => !existing.has(turn.turnId));
  return {
    ...state,
    turns: [...older, ...state.turns],
    session: { ...state.session, hasMore: event.has_more },
  };
}
```

Map the remaining `cancelled`, `ask_resolved`, `ask_rejected`, and `plan_resolved` cases to the new queue. `cancelled` clears all pending interactions for the last turn. Initialize every new field in `createRawSessionState` and keep `pendingInteraction` synchronized only as the explicitly temporary build-compatibility mirror.

- [ ] **Step 5: Run reducer tests and the transcript projection tests**

```powershell
pnpm exec vitest run src/store/sessionEventReducer.test.ts src/presentation/useConversationView.test.ts src/presentation/turnProjection.test.ts
pnpm build
```

Expected: all selected tests and the TypeScript/Vite build pass; restore and live projections retain reasoning, tools, answers, usage, and interactions.

- [ ] **Step 6: Commit the complete raw event model**

```powershell
cd F:\DeepX-Fork
git add crates/deepx-tauri/src/store/rawSession.ts crates/deepx-tauri/src/store/sessionEventReducer.ts crates/deepx-tauri/src/store/sessionEventReducer.test.ts
git commit -m "refactor(tauri): complete raw session event model"
git hash-object crates/deepx-tauri/src-tauri/Cargo.toml
```

Expected hash: `5b80caae02abced664f6801fbd98fb512e3d979e`.

---

### Task 2: Replace legacy signals with pure session selectors

**Files:**
- Create: `crates/deepx-tauri/src/store/sessionSelectors.ts`
- Create: `crates/deepx-tauri/src/store/sessionSelectors.test.ts`

**Interfaces:**
- Consumes: `RawSessionState`, `RawTurn`, and `PendingInteraction` from Task 1.
- Produces: `activeTurn`, `isSessionStreaming`, `activeInteraction`, `sessionUsage`, `failedPrompt`, and `canLoadMore`.

- [ ] **Step 1: Write failing selector tests**

Create `sessionSelectors.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { createRawSessionState } from "./sessionEventReducer";
import {
  activeInteraction, activeTurn, canLoadMore, failedPrompt,
  isSessionStreaming, sessionUsage,
} from "./sessionSelectors";

describe("sessionSelectors", () => {
  it("derives active state, gate, usage, retry text, and pagination", () => {
    const state = createRawSessionState("seed-a");
    state.turns.push({
      turnId: "t1", userText: "retry me", status: "failed", rounds: [], interactions: [],
    });
    state.turns.push({
      turnId: "t2", userText: "active", status: "waiting", rounds: [], interactions: [],
    });
    state.pendingInteractions.push({
      kind: "plan", id: "p1", turnId: "t2", content: "# Plan",
    });
    state.session.hasMore = true;
    state.session.usage = {
      prompt_tokens: 80, completion_tokens: 20, total_tokens: 100,
      prompt_cache_hit_tokens: 60, prompt_cache_miss_tokens: 20, reasoning_tokens: 5,
    };

    expect(activeTurn(state)?.turnId).toBe("t2");
    expect(isSessionStreaming(state)).toBe(true);
    expect(activeInteraction(state)?.id).toBe("p1");
    expect(sessionUsage(state)).toMatchObject({ contextTokens: 80, totalTokens: 100 });
    expect(failedPrompt(state)).toBe("retry me");
    expect(canLoadMore(state)).toBe(true);
  });
});
```

- [ ] **Step 2: Run the selector test and verify the module is missing**

```powershell
cd F:\DeepX-Fork\crates\deepx-tauri
pnpm exec vitest run src/store/sessionSelectors.test.ts
```

Expected: FAIL because `sessionSelectors.ts` does not exist.

- [ ] **Step 3: Implement the selectors as a complete module**

Create `sessionSelectors.ts`:

```ts
import type { PendingInteraction, RawSessionState, RawTurn } from "./rawSession";

export function activeTurn(state: RawSessionState): RawTurn | undefined {
  return [...state.turns].reverse().find(turn =>
    turn.status === "running" || turn.status === "waiting",
  );
}

export function isSessionStreaming(state: RawSessionState): boolean {
  return activeTurn(state) !== undefined;
}

export function activeInteraction(state: RawSessionState): PendingInteraction | null {
  return state.pendingInteractions[0] ?? null;
}

export function sessionUsage(state: RawSessionState) {
  const usage = state.session.usage;
  return {
    contextTokens: usage?.prompt_tokens ?? state.session.tokensUsed,
    totalTokens: usage?.total_tokens ?? state.session.tokensUsed,
    cacheHit: usage?.prompt_cache_hit_tokens ?? 0,
    cacheMiss: usage?.prompt_cache_miss_tokens ?? 0,
    contextLimit: state.session.contextLimit,
    model: state.session.model ?? "",
  };
}

export function failedPrompt(state: RawSessionState): string | null {
  return [...state.turns].reverse().find(turn => turn.status === "failed")?.userText ?? null;
}

export function canLoadMore(state: RawSessionState): boolean {
  return state.session.hasMore && state.turns.length > 0;
}
```

- [ ] **Step 4: Run the selector and reducer tests**

```powershell
pnpm exec vitest run src/store/sessionSelectors.test.ts src/store/sessionEventReducer.test.ts
```

Expected: both files pass.

- [ ] **Step 5: Commit selectors**

```powershell
cd F:\DeepX-Fork
git add crates/deepx-tauri/src/store/sessionSelectors.ts crates/deepx-tauri/src/store/sessionSelectors.test.ts
git commit -m "refactor(tauri): derive session ui state"
```

---

### Task 3: Type and harden the event boundary, replay, and reload snapshot

**Files:**
- Create: `crates/deepx-tauri/src/runtime/agentEventBoundary.ts`
- Create: `crates/deepx-tauri/src/runtime/agentEventBoundary.test.ts`
- Modify: `crates/deepx-tauri/src/runtime/sessionReplayBuffer.ts`
- Modify: `crates/deepx-tauri/src/runtime/sessionReplayBuffer.test.ts`
- Modify: `crates/deepx-tauri/src/store/sessionEventRuntime.ts`
- Modify: `crates/deepx-tauri/src/store/sessionEventRuntime.test.ts`

**Interfaces:**
- Consumes: generated `Agent2Ui` and `RawSessionState` from Task 1.
- Produces: `parseAgentEvent(payload: unknown): Agent2Ui`, typed `SessionReplayBuffer`, and reload snapshot schema version 2.

- [ ] **Step 1: Add failing boundary and typed replay tests**

Create `agentEventBoundary.test.ts`:

```ts
import { expect, it } from "vitest";
import { parseAgentEvent } from "./agentEventBoundary";

it("accepts a generated event and rejects malformed or unknown discriminants", () => {
  expect(parseAgentEvent({ type: "ready" })).toEqual({ type: "ready" });
  expect(() => parseAgentEvent(null)).toThrow("agent event must be an object");
  expect(() => parseAgentEvent({ type: "future_event" })).toThrow("unknown Agent2Ui event type");
});
```

Replace the replay overlap test with typed events:

Add `import type { Agent2Ui } from "../lib/types";` to the replay test and `import type { RawSessionState } from "./rawSession";` to the runtime test.

```ts
it("applies typed replay before live events and removes one exact overlap", () => {
  const buffer = createSessionReplayBuffer();
  const applied: Agent2Ui["type"][] = [];
  const apply = (event: Agent2Ui) => applied.push(event.type);
  const overlap: Agent2Ui = { type: "done" };
  buffer.begin("seed-a");
  buffer.handleLive("seed-a", overlap, apply);
  buffer.handleLive("seed-a", { type: "cancelled" }, apply);
  buffer.complete("seed-a", [{ type: "ready" }, overlap], apply);
  expect(applied).toEqual(["ready", "done", "cancelled"]);
});
```

Add runtime coverage:

```ts
it("removes a version-1 snapshot and commits when persistence throws", () => {
  const values = new Map<string, string>();
  values.set("deepx:reload:v1:seed-a", JSON.stringify({
    version: 1, state: createRawSessionState("seed-a"),
  }));
  const commits: RawSessionState[] = [];
  const storage: ReloadStorage = {
    getItem: key => values.get(key) ?? null,
    setItem: () => { throw new Error("quota"); },
    removeItem: key => { values.delete(key); },
  };
  expect(loadReloadSnapshot(storage, "seed-a")).toBeUndefined();
  expect(values.has("deepx:reload:v1:seed-a")).toBe(false);
  const runtime = createSessionEventRuntime({
    initialState: createRawSessionState("seed-a"), commit: state => commits.push(state), storage,
  });
  runtime.push({ type: "ready" });
  expect(commits.at(-1)?.session.ready).toBe(true);
});
```

- [ ] **Step 2: Run focused tests and verify failures**

```powershell
cd F:\DeepX-Fork\crates\deepx-tauri
pnpm exec vitest run src/runtime/agentEventBoundary.test.ts src/runtime/sessionReplayBuffer.test.ts src/store/sessionEventRuntime.test.ts
```

Expected: missing boundary module, replay type mismatches, and the old snapshot version assertion fails.

- [ ] **Step 3: Implement the generated-discriminant boundary**

Create `agentEventBoundary.ts`:

```ts
import type { Agent2Ui } from "../lib/types";

const EVENT_TYPES: ReadonlySet<Agent2Ui["type"]> = new Set([
  "turn_start", "turn_end", "round_delta", "round_complete", "tool_results",
  "tool_exec_delta", "session_restored", "more_turns", "session_created", "error",
  "tool_notice", "plan_submitted", "plan_resolved", "dashboard", "done",
  "compact_start", "compact_end", "compact_delta", "cancelled", "shutdown_ack",
  "ready", "audit_record", "exec_progress", "tool_call_preview", "code_delta",
  "pong", "skills_changed", "permission_request", "ask_user", "ask_resolved",
  "ask_rejected",
]);

export function parseAgentEvent(payload: unknown): Agent2Ui {
  if (typeof payload !== "object" || payload === null || Array.isArray(payload)) {
    throw new Error("agent event must be an object");
  }
  const type = (payload as { type?: unknown }).type;
  if (typeof type !== "string" || !EVENT_TYPES.has(type as Agent2Ui["type"])) {
    throw new Error(`unknown Agent2Ui event type: ${String(type)}`);
  }
  return payload as Agent2Ui;
}
```

- [ ] **Step 4: Make replay fully typed and bump snapshot schema**

In `sessionReplayBuffer.ts`, replace the record aliases with:

```ts
import type { Agent2Ui } from "../lib/types";

export type ApplySessionEvent = (event: Agent2Ui) => void;

export interface SessionReplayBuffer {
  begin(seed: string): void;
  handleLive(seed: string, event: Agent2Ui, apply: ApplySessionEvent): void;
  complete(seed: string, replayed: Agent2Ui[], apply: ApplySessionEvent): void;
  abort(seed: string, apply: ApplySessionEvent): void;
  clear(): void;
}
```

Change every replay collection and `eventSignature` parameter to `Agent2Ui`. Keep occurrence-counted exact JSON signatures so two genuinely repeated deltas are not collapsed globally.

In `sessionEventRuntime.ts`, set `SNAPSHOT_VERSION = 2` and `SNAPSHOT_PREFIX = "deepx:reload:v2:"`. Before reading v2, remove `deepx:reload:v1:${seed}` so incompatible snapshots do not accumulate. Keep `commitAndPersist` ordering as commit first, storage second; keep the existing storage `try/catch`. Ensure `dispose()` flushes before setting `disposed = true`.

- [ ] **Step 5: Run boundary, replay, runtime, and protocol-binding tests**

```powershell
pnpm exec vitest run src/runtime/agentEventBoundary.test.ts src/runtime/sessionReplayBuffer.test.ts src/store/sessionEventRuntime.test.ts src/lib/types/protocolBindings.test.ts
```

Expected: all selected tests pass, and the copied binding remains identical to the generated binding.

- [ ] **Step 6: Commit the typed reload boundary**

```powershell
cd F:\DeepX-Fork
git add crates/deepx-tauri/src/runtime/agentEventBoundary.ts crates/deepx-tauri/src/runtime/agentEventBoundary.test.ts crates/deepx-tauri/src/runtime/sessionReplayBuffer.ts crates/deepx-tauri/src/runtime/sessionReplayBuffer.test.ts crates/deepx-tauri/src/store/sessionEventRuntime.ts crates/deepx-tauri/src/store/sessionEventRuntime.test.ts
git commit -m "refactor(tauri): type session replay boundary"
```

---

### Task 4: Introduce local UI state and one per-session registry

**Files:**
- Create: `crates/deepx-tauri/src/store/sessionUiState.ts`
- Create: `crates/deepx-tauri/src/store/sessionUiState.test.ts`
- Create: `crates/deepx-tauri/src/store/sessionRegistry.ts`
- Create: `crates/deepx-tauri/src/store/sessionRegistry.test.ts`

**Interfaces:**
- Consumes: `createRawSessionState`, `loadReloadSnapshot`, `removeReloadSnapshot`, and `createSessionEventRuntime`.
- Produces: `SessionUiState`, `SessionEntry`, and `createSessionRegistry({ storage })` with `ensure`, `get`, `findByListenerSeed`, `remap`, `remove`, `entries`, and `disposeView`.

- [ ] **Step 1: Write failing UI-state and registry tests**

Create `sessionUiState.test.ts`:

```ts
import { createRoot } from "solid-js";
import { expect, it } from "vitest";
import { createSessionUiState } from "./sessionUiState";

it("owns workspace and rejects a duplicate interaction submission", () => {
  createRoot(dispose => {
    const ui = createSessionUiState();
    ui.setWorkspace("F:\\repo-a");
    expect(ui.workspace()).toBe("F:\\repo-a");
    expect(ui.beginInteractionSubmit("ask-1")).toBe(true);
    expect(ui.beginInteractionSubmit("ask-1")).toBe(false);
    ui.finishInteractionSubmit("ask-1");
    expect(ui.submittingInteractionId()).toBeNull();
    dispose();
  });
});
```

Create `sessionRegistry.test.ts`:

```ts
import { expect, it, vi } from "vitest";
import { createRawSessionState } from "./sessionEventReducer";
import { createSessionRegistry } from "./sessionRegistry";

function memoryStorage() {
  const values = new Map<string, string>();
  return {
    values,
    storage: {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => { values.set(key, value); },
      removeItem: (key: string) => { values.delete(key); },
    },
  };
}

it("hydrates once, remaps without replacing the entry, and removes frontend resources", () => {
  const { values, storage } = memoryStorage();
  const restored = createRawSessionState("old");
  restored.turns.push({
    turnId: "t1", userText: "restored", status: "completed", rounds: [], interactions: [],
  });
  values.set("deepx:reload:v2:old", JSON.stringify({ version: 2, state: restored }));

  const registry = createSessionRegistry({ storage });
  const entry = registry.ensure("old");
  const unlisten = vi.fn();
  entry.attachListener(unlisten);

  expect(registry.ensure("old")).toBe(entry);
  expect(entry.state().turns[0].turnId).toBe("t1");
  expect(registry.remap("old", "new")).toBe(entry);
  expect(entry.state().seed).toBe("new");
  expect(entry.state().turns[0].turnId).toBe("t1");

  registry.remove("new");
  expect(unlisten).toHaveBeenCalledOnce();
  expect(registry.get("new")).toBeUndefined();
  expect(values.has("deepx:reload:v2:old")).toBe(false);
  expect(values.has("deepx:reload:v2:new")).toBe(false);
});

it("disposes only frontend-owned runtimes and listeners", () => {
  const { storage } = memoryStorage();
  const registry = createSessionRegistry({ storage });
  const entry = registry.ensure("seed-a");
  const unlisten = vi.fn();
  entry.attachListener(unlisten);
  registry.disposeView();
  expect(unlisten).toHaveBeenCalledOnce();
  expect(registry.entries()).toEqual([]);
});
```

- [ ] **Step 2: Run focused tests and verify both modules are missing**

```powershell
cd F:\DeepX-Fork\crates\deepx-tauri
pnpm exec vitest run src/store/sessionUiState.test.ts src/store/sessionRegistry.test.ts
```

Expected: FAIL because both modules do not exist.

- [ ] **Step 3: Implement the complete local UI state module**

Create `sessionUiState.ts`:

```ts
import { createSignal, type Accessor } from "solid-js";

export interface SessionUiState {
  workspace: Accessor<string>;
  setWorkspace(value: string): void;
  submittingInteractionId: Accessor<string | null>;
  beginInteractionSubmit(id: string): boolean;
  finishInteractionSubmit(id: string): void;
}

export function createSessionUiState(): SessionUiState {
  const [workspace, setWorkspaceSignal] = createSignal("");
  const [submittingInteractionId, setSubmittingInteractionId] = createSignal<string | null>(null);
  return {
    workspace,
    setWorkspace: setWorkspaceSignal,
    submittingInteractionId,
    beginInteractionSubmit(id) {
      if (!id || submittingInteractionId() !== null) return false;
      setSubmittingInteractionId(id);
      return true;
    },
    finishInteractionSubmit(id) {
      if (submittingInteractionId() === id) setSubmittingInteractionId(null);
    },
  };
}
```

- [ ] **Step 4: Implement the registry with atomic seed remapping**

Create `sessionRegistry.ts` around this public contract:

```ts
import { createSignal, type Accessor } from "solid-js";
import { createRawSessionState } from "./sessionEventReducer";
import type { RawSessionState } from "./rawSession";
import { createSessionUiState, type SessionUiState } from "./sessionUiState";
import {
  createSessionEventRuntime, loadReloadSnapshot, removeReloadSnapshot,
  type ReloadStorage, type SessionEventRuntime,
} from "./sessionEventRuntime";

export interface SessionEntry {
  listenerSeed: string;
  state: Accessor<RawSessionState>;
  runtime: SessionEventRuntime;
  ui: SessionUiState;
  hasListener(): boolean;
  attachListener(unlisten: () => void): void;
  detachListener(): void;
}

export function createSessionRegistry(options: { storage: ReloadStorage }) {
  const bySeed = new Map<string, SessionEntry>();

  function ensure(seed: string): SessionEntry {
    const existing = get(seed);
    if (existing) return existing;
    const initial = loadReloadSnapshot(options.storage, seed) ?? createRawSessionState(seed);
    const [state, setState] = createSignal(initial);
    let unlisten: (() => void) | undefined;
    const entry: SessionEntry = {
      listenerSeed: seed,
      state,
      runtime: createSessionEventRuntime({
        initialState: initial,
        commit: setState,
        storage: options.storage,
      }),
      ui: createSessionUiState(),
      hasListener: () => unlisten !== undefined,
      attachListener(next) { unlisten?.(); unlisten = next; },
      detachListener() { const current = unlisten; unlisten = undefined; current?.(); },
    };
    bySeed.set(seed, entry);
    return entry;
  }

  function get(seed: string): SessionEntry | undefined {
    return bySeed.get(seed) ?? [...bySeed.values()].find(entry => entry.state().seed === seed);
  }

  function findByListenerSeed(seed: string): SessionEntry | undefined {
    return [...bySeed.values()].find(entry => entry.listenerSeed === seed);
  }

  function remap(listenerSeed: string, nextSeed: string): SessionEntry {
    const entry = findByListenerSeed(listenerSeed) ?? ensure(listenerSeed);
    const oldSeed = entry.state().seed;
    entry.runtime.update(state => ({ ...state, seed: nextSeed }));
    bySeed.delete(oldSeed);
    bySeed.delete(listenerSeed);
    bySeed.set(nextSeed, entry);
    removeReloadSnapshot(options.storage, oldSeed);
    if (listenerSeed !== oldSeed) removeReloadSnapshot(options.storage, listenerSeed);
    return entry;
  }

  function remove(seed: string): void {
    const entry = get(seed);
    if (!entry) return;
    entry.detachListener();
    entry.runtime.dispose();
    bySeed.delete(seed);
    bySeed.delete(entry.listenerSeed);
    removeReloadSnapshot(options.storage, seed);
    removeReloadSnapshot(options.storage, entry.listenerSeed);
  }

  function disposeView(): void {
    for (const entry of new Set(bySeed.values())) {
      entry.runtime.dispose();
      entry.detachListener();
    }
    bySeed.clear();
  }

  return { ensure, get, findByListenerSeed, remap, remove, entries: () => [...new Set(bySeed.values())], disposeView };
}
```

If the registry test reveals that `remap` persists the old seed under the new key before the state update is committed, fix `sessionEventRuntime.update` ordering rather than adding a second raw signal.

- [ ] **Step 5: Run registry, reload, and cleanup tests**

```powershell
pnpm exec vitest run src/store/sessionUiState.test.ts src/store/sessionRegistry.test.ts src/store/sessionEventRuntime.test.ts src/runtime/viewLifecycle.test.ts
```

Expected: all selected files pass; no test invokes a backend close command.

- [ ] **Step 6: Commit the registry boundary**

```powershell
cd F:\DeepX-Fork
git add crates/deepx-tauri/src/store/sessionUiState.ts crates/deepx-tauri/src/store/sessionUiState.test.ts crates/deepx-tauri/src/store/sessionRegistry.ts crates/deepx-tauri/src/store/sessionRegistry.test.ts
git commit -m "refactor(tauri): own sessions in one registry"
```

---

### Task 5: Make pagination real and plan review presentation-only

**Files:**
- Modify: `crates/deepx-tauri/src/components/PlanReviewPanel.tsx`
- Modify: `crates/deepx-tauri/src/components/PlanReviewPanel.test.tsx`
- Modify: `crates/deepx-tauri/src/components/shell/ThreadHeader.tsx`
- Modify: `crates/deepx-tauri/src/components/shell/ThreadHeader.test.tsx`
- Modify: `crates/deepx-tauri/src/components/conversation/ConversationTranscript.tsx`
- Create: `crates/deepx-tauri/src/components/conversation/ConversationTranscript.test.tsx`

**Interfaces:**
- Consumes: existing plan-review callbacks and `TurnViewModel[]`.
- Produces: one-command plan review callbacks and optional pagination props that keep current `ChatView` buildable until the atomic Task 7 switch.

- [ ] **Step 1: Add failing one-command and pagination tests**

Replace the approve test in `PlanReviewPanel.test.tsx` with:

```tsx
it("delegates approval exactly once without invoking Tauri itself", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const onApprove = vi.fn().mockResolvedValue(undefined);
  dispose = render(() => (
    <PlanReviewPanel
      seed="seed-1"
      callId="call-1"
      planContent="test plan"
      onApprove={onApprove}
      onReject={vi.fn()}
      onClose={vi.fn()}
    />
  ), host);

  host.querySelector<HTMLButtonElement>(".interaction-approve")!.click();
  await flush();

  expect(onApprove).toHaveBeenCalledOnce();
  expect(invoke).not.toHaveBeenCalled();
});
```

Create `ConversationTranscript.test.tsx`:

```tsx
// @vitest-environment jsdom
import { createSignal } from "solid-js";
import { render } from "solid-js/web";
import { expect, it, vi } from "vitest";
import type { TurnViewModel } from "../../presentation/turnProjection";
import ConversationTranscript from "./ConversationTranscript";

it("loads older turns from a real transcript control", () => {
  const host = document.createElement("div");
  const onLoadMore = vi.fn();
  const dispose = render(() => (
    <ConversationTranscript turns={[]} hasMore={true} onLoadMore={onLoadMore} />
  ), host);
  host.querySelector<HTMLButtonElement>("[data-load-more]")!.click();
  expect(onLoadMore).toHaveBeenCalledOnce();
  dispose();
});

it("preserves viewport distance when older turns prepend", async () => {
  const host = document.createElement("div");
  const [turns, setTurns] = createSignal<TurnViewModel[]>([{
    turnId: "new", userPrompt: "new", status: "completed", rounds: [], interactions: [],
  }]);
  let height = 1000;
  const dispose = render(() => <ConversationTranscript
    turns={turns()} hasMore={true}
    onLoadMore={() => {
      height = 1200;
      setTurns(current => [{
        turnId: "old", userPrompt: "old", status: "completed", rounds: [], interactions: [],
      }, ...current]);
    }}
  />, host);
  const scroller = host.querySelector<HTMLElement>(".conversation-scroll")!;
  Object.defineProperty(scroller, "scrollHeight", { get: () => height });
  scroller.scrollTop = 400;
  host.querySelector<HTMLButtonElement>("[data-load-more]")!.click();
  await Promise.resolve();
  await Promise.resolve();
  expect(scroller.scrollTop).toBe(600);
  dispose();
});
```

- [ ] **Step 2: Run focused tests and verify the duplicated command and missing button**

```powershell
cd F:\DeepX-Fork\crates\deepx-tauri
pnpm exec vitest run src/components/PlanReviewPanel.test.tsx src/components/conversation/ConversationTranscript.test.tsx
```

Expected: plan approval still calls `invoke`, and the transcript has no `[data-load-more]` control.

- [ ] **Step 3: Remove Tauri command ownership from PlanReviewPanel**

Remove the `invoke` import. Keep `seed` and `callId` as optional transitional props through Task 6 so the current `ChatView` compiles, but do not read them. Change callback types and handlers to:

```ts
interface PlanReviewPanelProps {
  seed?: string;
  callId?: string;
  planContent: string;
  onApprove: () => void | Promise<void>;
  onReject: (message?: string) => void | Promise<void>;
  onClose: () => void;
}

async function handleApprove() {
  if (busy()) return;
  setBusy(true);
  try { await props.onApprove(); }
  finally { setBusy(false); }
}

async function handleReject() {
  if (busy()) return;
  const message = feedback().trim() || undefined;
  setBusy(true);
  try { await props.onReject(message); }
  finally { setBusy(false); }
}
```

Keep the test's mocked `invoke` import as a regression assertion that this presentation component never regains command ownership.

- [ ] **Step 4: Add backward-compatible transcript pagination**

Change props to optional until Task 7 supplies them authoritatively:

```ts
export default function ConversationTranscript(props: {
  turns: TurnViewModel[];
  hasMore?: boolean;
  onLoadMore?: () => void | Promise<void>;
})
```

Render this before `<main class="conversation-transcript">`:

```tsx
<Show when={props.hasMore && props.onLoadMore}>
  <button
    type="button"
    data-load-more
    class="load-more-turns"
    onClick={() => void props.onLoadMore?.()}
  >加载更早消息</button>
</Show>
```

Extend the existing length effect so a first-id change caused by prepending older turns preserves the current viewport; only a true session replacement or append while near-bottom scrolls to the bottom. Store the prior last turn id as the session identity signal rather than treating every first-id change as a session switch.

Use:

```ts
let prevLen = 0;
let prevLastId = "";

async function loadOlder() {
  if (!props.onLoadMore) return;
  const distanceFromBottom = scroller.scrollHeight - scroller.scrollTop;
  await props.onLoadMore();
  queueMicrotask(() => {
    scroller.scrollTop = Math.max(0, scroller.scrollHeight - distanceFromBottom);
  });
}

createEffect(() => {
  const len = props.turns.length;
  const lastId = props.turns[len - 1]?.turnId ?? "";
  const firstRender = prevLastId === "";
  const prepended = len > prevLen && lastId === prevLastId;
  const appended = len > prevLen && lastId !== prevLastId;
  const replaced = !firstRender && !prepended && lastId !== prevLastId;
  if (firstRender || replaced || (appended && nearBottom())) {
    queueMicrotask(() => scroller?.scrollTo({ top: scroller.scrollHeight }));
  }
  prevLen = len;
  prevLastId = lastId;
});
```

Bind the pagination button to `onClick={() => void loadOlder()}`.

- [ ] **Step 5: Run focused tests and the full build**

```powershell
pnpm exec vitest run src/components/PlanReviewPanel.test.tsx src/components/conversation/ConversationTranscript.test.tsx src/components/conversation/TurnGroup.test.tsx
pnpm build
```

Expected: all tests and `tsc --noEmit && vite build` pass; current App/ChatView remain buildable.

- [ ] **Step 6: Commit the independent presentation fixes**

```powershell
cd F:\DeepX-Fork
git add crates/deepx-tauri/src/components/PlanReviewPanel.tsx crates/deepx-tauri/src/components/PlanReviewPanel.test.tsx crates/deepx-tauri/src/components/conversation/ConversationTranscript.tsx crates/deepx-tauri/src/components/conversation/ConversationTranscript.test.tsx
git commit -m "refactor(tauri): isolate conversation presentation"
```

---

### Task 6: Add one exhaustive event dispatcher

**Files:**
- Create: `crates/deepx-tauri/src/runtime/agentEventDispatcher.ts`
- Create: `crates/deepx-tauri/src/runtime/agentEventDispatcher.test.ts`

**Interfaces:**
- Consumes: `Agent2Ui`, a `push(event)` reducer sink, and explicit side-effect callbacks.
- Produces: `dispatchAgentEvent(event, target, effects)` with exhaustive event handling and isolated reducer failures.

- [ ] **Step 1: Write failing dispatcher tests**

Create `agentEventDispatcher.test.ts`:

```ts
import { expect, it, vi } from "vitest";
import type { Agent2Ui } from "../lib/types";
import { dispatchAgentEvent, type AgentEventEffects } from "./agentEventDispatcher";

function effects(): AgentEventEffects {
  return {
    onSessionCreated: vi.fn(), onSessionRestored: vi.fn(), onDashboard: vi.fn(),
    onError: vi.fn(), onCancelled: vi.fn(), onInteractionSettled: vi.fn(),
    onReducerError: vi.fn(),
  };
}

it("pushes every event before running its explicit side effect", () => {
  const target = { push: vi.fn() };
  const fx = effects();
  const created: Agent2Ui = { type: "session_created", seed: "seed-a" };
  dispatchAgentEvent(created, target, fx);
  expect(target.push).toHaveBeenCalledWith(created);
  expect(fx.onSessionCreated).toHaveBeenCalledWith("seed-a");

  const error: Agent2Ui = { type: "error", message: "lost" };
  dispatchAgentEvent(error, target, fx);
  expect(fx.onError).toHaveBeenCalledWith("lost");

  dispatchAgentEvent({ type: "ask_rejected", ask_id: "ask-1", message: "retry" }, target, fx);
  expect(fx.onInteractionSettled).toHaveBeenCalledWith("ask-1");
  expect(fx.onError).toHaveBeenCalledWith("retry");

  dispatchAgentEvent({ type: "ready" }, target, fx);
  expect(fx.onDashboard).not.toHaveBeenCalled();
});

it("isolates reducer failure and skips side effects for the failed event", () => {
  const failure = new Error("bad payload");
  const target = { push: vi.fn(() => { throw failure; }) };
  const fx = effects();
  const event: Agent2Ui = { type: "session_restored", seed: "seed-a", turns: [], tokens_used: 0, cache_hit_pct: 0, total_turns: 0, has_more: false };
  dispatchAgentEvent(event, target, fx);
  expect(fx.onReducerError).toHaveBeenCalledWith(event, failure);
  expect(fx.onSessionRestored).not.toHaveBeenCalled();
});
```

The production switch's `const unreachable: never = event` is the compile-time exhaustiveness gate for every generated variant; the tests cover ordering, routed effects, no-op effects, and failure isolation.

- [ ] **Step 2: Run the dispatcher test and verify the module is missing**

```powershell
cd F:\DeepX-Fork\crates\deepx-tauri
pnpm exec vitest run src/runtime/agentEventDispatcher.test.ts
```

Expected: FAIL because `agentEventDispatcher.ts` does not exist.

- [ ] **Step 3: Implement the dispatcher and explicit side effects**

Create `agentEventDispatcher.ts`:

```ts
import type { Agent2Ui } from "../lib/types";

export interface AgentEventEffects {
  onSessionCreated(seed: string): void;
  onSessionRestored(seed: string): void;
  onDashboard(): void;
  onError(message: string): void;
  onCancelled(): void;
  onInteractionSettled(id: string): void;
  onReducerError(event: Agent2Ui, error: unknown): void;
}

export function dispatchAgentEvent(
  event: Agent2Ui,
  target: { push(event: Agent2Ui): void },
  effects: AgentEventEffects,
): void {
  try {
    target.push(event);
  } catch (error) {
    effects.onReducerError(event, error);
    return;
  }

  switch (event.type) {
    case "session_created": effects.onSessionCreated(event.seed); return;
    case "session_restored": effects.onSessionRestored(event.seed); return;
    case "dashboard": effects.onDashboard(); return;
    case "error": effects.onError(event.message); return;
    case "cancelled": effects.onCancelled(); return;
    case "ask_resolved": effects.onInteractionSettled(event.ask_id); return;
    case "ask_rejected":
      effects.onInteractionSettled(event.ask_id);
      effects.onError(event.message);
      return;
    case "plan_resolved": effects.onInteractionSettled(event.call_id); return;
    case "turn_start": case "turn_end": case "round_delta": case "round_complete":
    case "tool_results": case "tool_exec_delta": case "more_turns": case "tool_notice":
    case "plan_submitted": case "done": case "compact_start":
    case "compact_end": case "compact_delta": case "shutdown_ack": case "ready":
    case "audit_record": case "exec_progress": case "tool_call_preview": case "code_delta":
    case "pong": case "skills_changed": case "permission_request": case "ask_user":
      return;
    default: {
      const unreachable: never = event;
      throw new Error(`unhandled Agent2Ui side effect: ${JSON.stringify(unreachable)}`);
    }
  }
}
```

- [ ] **Step 4: Run dispatcher and binding tests**

```powershell
pnpm exec vitest run src/runtime/agentEventDispatcher.test.ts src/lib/types/protocolBindings.test.ts
```

Expected: all events are exhaustively represented and both files pass.

- [ ] **Step 5: Commit the dispatcher**

```powershell
cd F:\DeepX-Fork
git add crates/deepx-tauri/src/runtime/agentEventDispatcher.ts crates/deepx-tauri/src/runtime/agentEventDispatcher.test.ts
git commit -m "refactor(tauri): centralize agent event dispatch"
```

---

### Task 7: Replace App orchestration and mount the authoritative shell

**Files:**
- Modify: `crates/deepx-tauri/src/App.tsx`
- Modify: `crates/deepx-tauri/src/components/ChatView.tsx`
- Modify: `crates/deepx-tauri/src/components/ChatView.interactions.test.tsx`
- Modify: `crates/deepx-tauri/src/components/PlanReviewPanel.tsx`
- Modify: `crates/deepx-tauri/src/components/PlanReviewPanel.test.tsx`
- Modify: `crates/deepx-tauri/src/components/shell/AppShell.tsx`
- Modify: `crates/deepx-tauri/src/components/shell/AppShell.test.tsx`
- Modify: `crates/deepx-tauri/src/components/shell/TaskSidebar.tsx`
- Modify: `crates/deepx-tauri/src/components/shell/TaskSidebar.test.tsx`
- Modify: `crates/deepx-tauri/src/store/rawSession.ts`
- Modify: `crates/deepx-tauri/src/store/sessionEventReducer.ts`
- Modify: `crates/deepx-tauri/src/store/sessionEventRuntime.ts`
- Modify: `crates/deepx-tauri/src/store/sessionEventRuntime.test.ts`
- Modify: `crates/deepx-tauri/src/store/sessionRegistry.test.ts`

**Interfaces:**
- Consumes: Tasks 1-6 registry, typed event boundary/replay, dispatcher, current `ChatView`, and selectors.
- Produces: an atomically migrated raw-only `ChatView` and `App` with one session registry, one event path, one shell, and command callbacks that update only authoritative raw/local state.

- [ ] **Step 1: Rewrite ChatView and shell tests before changing production**

Replace legacy ChatStore/permission-queue fixtures in `ChatView.interactions.test.tsx` with this raw harness:

```tsx
function mountRawChat(initial: RawSessionState) {
  const [state, setState] = createSignal(initial);
  const ui = createSessionUiState();
  ui.setWorkspace("F:/repo");
  const callbacks = {
    onAskSubmit: vi.fn().mockResolvedValue(undefined),
    onAskDismiss: vi.fn().mockResolvedValue(undefined),
    onPermissionRespond: vi.fn().mockResolvedValue(undefined),
    onPlanRespond: vi.fn().mockResolvedValue(undefined),
    onTaskAction: vi.fn().mockResolvedValue(undefined),
    onLoadMore: vi.fn().mockResolvedValue(undefined),
  };
  const host = document.createElement("div");
  document.body.append(host);
  const i18n = createI18n("zh");
  cleanups.push(render(() => (
    <I18nCtx.Provider value={i18n}>
      <ChatView
        rawSession={state}
        ui={ui}
        onLoadMore={callbacks.onLoadMore}
        onAskSubmit={callbacks.onAskSubmit}
        onAskDismiss={callbacks.onAskDismiss}
        onPermissionRespond={callbacks.onPermissionRespond}
        onPlanRespond={callbacks.onPlanRespond}
        onTaskAction={callbacks.onTaskAction}
        onUndo={vi.fn()}
        permissionLevel={2}
        onPermissionLevelChange={vi.fn()}
        onChangeWorkspace={vi.fn()}
      />
    </I18nCtx.Provider>
  ), host));
  return { host, state, setState, ui, callbacks };
}

it("renders only the first raw interaction and forwards its typed id", async () => {
  const state = createRawSessionState("seed-1");
  state.pendingInteractions.push(
    {
      kind: "ask", id: "ask-1", turnId: "t1", roundNum: 0, mode: "single",
      questions: [{ id: "q1", question: "Continue?", options: ["yes"], allow_custom: false }],
    },
    { kind: "plan", id: "plan-1", turnId: "t1", content: "# Later" },
  );
  const { callbacks } = mountRawChat(state);
  const dialog = document.body.querySelector('[role="dialog"]')!;
  expect(dialog.textContent).toContain("Continue?");
  expect(dialog.textContent).not.toContain("Later");
  dialog.querySelector<HTMLButtonElement>(".interaction-option")!.click();
  dialog.querySelector<HTMLButtonElement>(".interaction-submit")!.click();
  await flush();
  expect(callbacks.onAskSubmit).toHaveBeenCalledWith(
    expect.objectContaining({ id: "ask-1", kind: "ask" }),
    [{ question_id: "q1", answer: "yes" }],
  );
});

it("renders raw permission and compact completion state", async () => {
  const state = createRawSessionState("seed-1");
  state.pendingInteractions.push({
    kind: "permission", id: "call-1", turnId: "t1", toolName: "exec_run",
    reason: "Run", paths: ["F:/repo"], category: "exec", level: 1,
    risk: "high", consequence: "May execute commands",
  });
  state.compact = { active: false, text: "", turnsCompacted: 8, completionRevision: 1 };
  const { callbacks, host } = mountRawChat(state);
  document.body.querySelector<HTMLButtonElement>(".approval-high")!.click();
  await flush();
  expect(callbacks.onPermissionRespond).toHaveBeenCalledWith(
    expect.objectContaining({ id: "call-1", kind: "permission" }), true, false,
  );
  expect(host.querySelector(".compact-complete")?.textContent).toContain("8 轮对话");
});
```

Use these shell assertions:

```tsx
it("mounts exactly one authoritative sidebar and workspace", () => {
  const host = document.createElement("div");
  const dispose = render(() => <AppShell
    sidebar={<aside data-task-sidebar />}
    workspace={<section data-active-workspace />}
  />, host);
  expect(host.querySelectorAll("[data-task-sidebar]")).toHaveLength(1);
  expect(host.querySelectorAll("[data-thread-workspace]")).toHaveLength(1);
  expect(host.querySelector(".sidebar")).toBeNull();
  dispose();
});

it("keeps all active TaskSidebar navigation", () => {
  const onNew = vi.fn();
  const onSkills = vi.fn();
  const onSettings = vi.fn();
  const onOpen = vi.fn();
  const host = document.createElement("div");
  const dispose = render(() => <TaskSidebar
    sessions={[session("seed-a")]}
    activeSeed="seed-a"
    onNew={onNew} onOpen={onOpen} onDelete={vi.fn()}
    onSkills={onSkills} onSettings={onSettings}
  />, host);
  const buttons = [...host.querySelectorAll<HTMLButtonElement>("button")];
  buttons.find(button => button.textContent?.includes("新建"))!.click();
  buttons.find(button => button.textContent?.includes("技能"))!.click();
  buttons.find(button => button.textContent?.includes("设置"))!.click();
  host.querySelector<HTMLButtonElement>(".task-row-main")!.click();
  expect(onNew).toHaveBeenCalledOnce();
  expect(onSkills).toHaveBeenCalledOnce();
  expect(onSettings).toHaveBeenCalledOnce();
  expect(onOpen).toHaveBeenCalledWith("seed-a");
  dispose();
});
```

Add this `ThreadHeader` regression:

Also add `undoDisabled={false}` and `onUndo={vi.fn()}` to the existing `shows explicit workspace and compaction actions` fixture so the final required prop contract is covered consistently.

```tsx
it("exposes authoritative undo and disables it while streaming", () => {
  const onUndo = vi.fn();
  const host = document.createElement("div");
  const dispose = render(() => <ThreadHeader
    title="Task" environmentOpen={false} statsOpen={false} workspace="F:/repo"
    compacting={false} undoDisabled={false}
    onToggleEnvironment={vi.fn()} onToggleStats={vi.fn()} onOpenLocation={vi.fn()}
    onChangeWorkspace={vi.fn()} onCompact={vi.fn()} onUndo={onUndo}
  />, host);
  host.querySelector<HTMLButtonElement>('[data-undo-turn]')!.click();
  expect(onUndo).toHaveBeenCalledOnce();
  dispose();
});
```

- [ ] **Step 2: Run shell tests and record the App integration baseline**

```powershell
cd F:\DeepX-Fork\crates\deepx-tauri
pnpm exec vitest run src/components/ChatView.interactions.test.tsx src/components/shell/AppShell.test.tsx src/components/shell/TaskSidebar.test.tsx src/components/shell/ThreadHeader.test.tsx
```

Expected: new ChatView tests fail to compile against legacy props; shell tests establish the one-shell assertions. The repository itself still builds before production changes.

- [ ] **Step 3: Atomically remove compatibility state and migrate ChatView**

Remove `pendingInteraction` from `RawSessionState`, `createRawSessionState`, and every reducer helper; `pendingInteractions` is now the only gate state. Update all reducer tests that asserted `pendingInteraction` to assert `pendingInteractions[0]` or the full queue. Bump `SNAPSHOT_VERSION` from 2 to 3, change the prefix to `deepx:reload:v3:`, remove both v1 and v2 keys before reading, and update runtime/registry snapshot tests to use v3 so old transitional snapshots are discarded safely.

Use this final `ChatView` prop contract:

```ts
interface ChatViewProps {
  rawSession: Accessor<RawSessionState>;
  ui: SessionUiState;
  onLoadMore: () => void | Promise<void>;
  onAskSubmit: (item: Extract<PendingInteraction, { kind: "ask" }>, answers: AskAnswer[]) => void | Promise<void>;
  onAskDismiss: (item: Extract<PendingInteraction, { kind: "ask" }>) => void | Promise<void>;
  onPermissionRespond: (
    item: Extract<PendingInteraction, { kind: "permission" }>,
    approved: boolean,
    trustFolder: boolean,
  ) => void | Promise<void>;
  onPlanRespond: (
    item: Extract<PendingInteraction, { kind: "plan" }>,
    approved: boolean,
    message?: string,
  ) => void | Promise<void>;
  onTaskAction: (action: "cancel" | "delete" | "ask", task: TaskInfo) => void | Promise<void>;
  onUndo: () => void | Promise<void>;
  permissionLevel: number;
  onPermissionLevelChange: (level: number) => void | Promise<void>;
  onChangeWorkspace: () => void | Promise<void>;
}
```

Replace every `chat()` read with raw/selectors/local UI access:

```ts
const session = () => props.rawSession();
const seed = () => session().seed;
const interaction = () => activeInteraction(session());
const permissionInteraction = () => {
  const item = interaction();
  return item?.kind === "permission" ? item : null;
};
const askInteraction = () => {
  const item = interaction();
  return item?.kind === "ask" ? item : null;
};
const planInteraction = () => {
  const item = interaction();
  return item?.kind === "plan" ? item : null;
};
const streaming = () => isSessionStreaming(session());
const usage = () => sessionUsage(session());
```

Use `props.ui.workspace()` for workspace, `session().dashboard.tasks` for tasks, `session().telemetry` for `ContextPanel.metricHistory`, `session().skills`, and `session().compact`. Pass `canLoadMore(session())` and `props.onLoadMore` to `ConversationTranscript`. Pass `streaming` to `ComposerDock.isStreaming` and `() => activeInteraction(session()) !== null` to `hasPendingGate`; use the same selector when deciding whether to drain follow-ups. Keep send/stop/compact/mode/git-branch invokes because they are commands, not state.

Add `onUndo` and `undoDisabled` to `ThreadHeader`, render an `撤销上一轮` button, and pass `undoDisabled={session().turns.length === 0 || streaming()}`. This makes the previously unreachable slash-menu undo behavior available in the authoritative shell before `SlashMenu` is deleted.

Render only the first typed interaction with separate `permissionInteraction`, `askInteraction`, and `planInteraction` accessors so TypeScript narrowing is stable. `PlanReviewPanel` becomes final presentation-only API `{ planContent, onApprove, onReject }`; remove its transitional `seed`, `callId`, and close button so a pending plan cannot be hidden without resolution.

Use this interaction switch:

```tsx
<Switch>
  <Match when={permissionInteraction()}>
    {item => <InteractionModal label="DeepX 请求操作授权">
      <PermissionPrompt
        request={{
          tool_call_id: item().id, tool_name: item().toolName,
          reason: item().reason, paths: item().paths, category: item().category,
          level: item().level, risk: item().risk, consequence: item().consequence,
        }}
        onRespond={(approved, trust) => void props.onPermissionRespond(item(), approved, trust)}
      />
    </InteractionModal>}
  </Match>
  <Match when={askInteraction()}>
    {item => <InteractionModal label="DeepX 需要你的回答">
      <AskUserPrompt
        questions={item().questions}
        onSubmit={answers => void props.onAskSubmit(item(), answers)}
        onDismiss={() => void props.onAskDismiss(item())}
      />
    </InteractionModal>}
  </Match>
  <Match when={planInteraction()}>
    {item => <InteractionModal label="审核执行计划">
      <PlanReviewPanel
        planContent={item().content}
        onApprove={() => props.onPlanRespond(item(), true)}
        onReject={message => props.onPlanRespond(item(), false, message)}
      />
    </InteractionModal>}
  </Match>
</Switch>
```

Update both `PlanReviewPanel.test.tsx` fixtures to remove `seed`, `callId`, and `onClose`, and assert `host.querySelector(".plan-review-close")` is null. Keep the mocked `invoke` assertion at zero calls.

Keep compact completion visibility local for four seconds using `completionRevision`, a `compactCompleteVisible` signal, one timeout, and `onCleanup`; the payload remains only in raw state.

- [ ] **Step 4: Replace parallel maps with the session registry**

Remove imports/types for `createChatStore`, `ChatStore`, `RawStore`, `createPermissionQueue`, `QueuedPermission`, `SlashCommand`, and the hidden sidebar/changelog. Create:

```ts
const registry = createSessionRegistry({ storage: sessionStorage });
const sessionReplay = createSessionReplayBuffer();
const pendingEntries = new Map<string, Promise<SessionEntry>>();

function activeEntry(): SessionEntry | undefined {
  const seed = activeSeed();
  return seed ? registry.get(seed) : undefined;
}

function activeRawSession(): RawSessionState | undefined {
  return activeEntry()?.state();
}
```

Implement `getOrCreateSessionEntry(seed)` so it:

1. returns one in-flight promise per seed;
2. calls `registry.ensure(seed)`;
3. registers `listen<unknown>(\`agent-${entry.listenerSeed}-event\`, ...)` before resume;
4. parses payload with `parseAgentEvent` before `sessionReplay.handleLive`;
5. routes applied events through `dispatchAgentEvent`;
6. attaches the returned unlisten function to the entry;
7. removes a failed entry with `registry.remove(seed)`.

Use:

```ts
async function getOrCreateSessionEntry(seed: string): Promise<SessionEntry> {
  const existing = registry.get(seed);
  if (existing?.hasListener()) return existing;
  const pending = pendingEntries.get(seed);
  if (pending) return pending;

  const creation = (async () => {
    const entry = registry.ensure(seed);
    try {
      const unlisten = await listen<unknown>(`agent-${entry.listenerSeed}-event`, event => {
        let parsed: Agent2Ui;
        try { parsed = parseAgentEvent(event.payload); }
        catch (error) {
          console.error("[App] ignored malformed live event", { seed: entry.listenerSeed, error });
          toastCtrl.push("收到无法识别的后端事件，已忽略", "error");
          return;
        }
        sessionReplay.handleLive(entry.listenerSeed, parsed, replayed => {
          handleAgentEvent(entry, replayed);
        });
      });
      entry.attachListener(unlisten);
      return entry;
    } catch (error) {
      registry.remove(seed);
      throw error;
    }
  })();
  pendingEntries.set(seed, creation);
  try { return await creation; }
  finally { pendingEntries.delete(seed); }
}
```

The listener callback must catch `parseAgentEvent` errors, log the listener seed, show one recoverable toast, and return without detaching the listener. A malformed payload must not enter replay or mutate the current raw state.

- [ ] **Step 5: Replace the old event switch with typed dispatcher effects**

Create one `handleAgentEvent(entry, event)` that calls `dispatchAgentEvent(event, entry.runtime, effects)`. Effects must:

- remap `session_created` through `registry.remap(entry.listenerSeed, seed)`, move `activeSeed` when appropriate, load workspace/dashboard, and refresh the session list;
- load workspace/dashboard and refresh sessions after `session_restored`;
- show error toasts and run the existing narrowly scoped dead-agent reconnect heuristic after reducer state has been preserved;
- clear no separate permission/ask/plan store on cancellation;
- clear the matching local submit guard on `ask_resolved`, `ask_rejected`, and `plan_resolved`; on cancellation, finish the currently submitting id without changing raw state outside the reducer;
- report reducer failures with seed and event type, show a recoverable toast, and keep listener/entry alive.

Delete the entire `switch (p.type)` that calls legacy handlers. No event may be reduced twice.

Construct the effects inline so they close over the exact `entry`. `onInteractionSettled` calls `entry.ui.finishInteractionSubmit(id)`. `onCancelled` reads `entry.ui.submittingInteractionId()` and finishes that id. `onReducerError` logs `{ seed: entry.state().seed, type: event.type, error }` and pushes a toast without removing the entry. Session-created/restored callbacks invoke dedicated async helpers with `void` so reducer delivery remains synchronous.

```ts
function handleAgentEvent(entry: SessionEntry, event: Agent2Ui) {
  dispatchAgentEvent(event, entry.runtime, {
    onSessionCreated: seed => { void afterSessionCreated(entry, seed); },
    onSessionRestored: seed => { void afterSessionRestored(entry, seed); },
    onDashboard: () => {},
    onError: message => { void handleAgentError(entry, message); },
    onCancelled: () => {
      const id = entry.ui.submittingInteractionId();
      if (id) entry.ui.finishInteractionSubmit(id);
    },
    onInteractionSettled: id => entry.ui.finishInteractionSubmit(id),
    onReducerError: (failedEvent, error) => {
      console.error("[App] reducer rejected event", {
        seed: entry.state().seed, type: failedEvent.type, error,
      });
      toastCtrl.push("会话事件处理失败，现有消息已保留", "error");
    },
  });
}
```

`afterSessionCreated` performs registry remap, active-seed update, workspace/dashboard load, local-storage seed write, and session-list refresh in that order. `afterSessionRestored` performs workspace/dashboard load, local-storage seed write, and session-list refresh without clearing raw turns. `handleAgentError` pushes the message and runs the existing dead-agent regex/reconnect branch only when it matches.

- [ ] **Step 6: Make resume/replay typed and listener-first**

Use this order in `resumeSession`:

```ts
sessionReplay.begin(seed);
let entry: SessionEntry | undefined;
try {
  entry = await getOrCreateSessionEntry(seed);
  await invoke("cmd_resume_session", { seed });
  const rawReplay = await invoke<unknown[]>("cmd_replay_session_events", { seed }).catch(() => []);
  const replayed = rawReplay.flatMap(payload => {
    try { return [parseAgentEvent(payload)]; }
    catch (error) {
      console.error("[App] ignored malformed replay event", { seed, error });
      return [];
    }
  });
  sessionReplay.complete(seed, replayed, event => handleAgentEvent(entry!, event));
  const currentSeed = entry.state().seed;
  localStorage.setItem(LS_KEY, currentSeed);
  setActiveSeed(currentSeed);
  setHasChosenSession(true);
  setView("chat");
} catch (error) {
  if (entry) sessionReplay.abort(seed, event => handleAgentEvent(entry!, event));
  else sessionReplay.abort(seed, () => {});
  console.error("[App] resumeSession error", error);
  if (!entry?.state().turns.length) {
    setHasChosenSession(false);
    setView("home");
  }
}
```

An existing attached entry may skip listener creation but must still execute resume plus lifecycle replay after a WebView reconstruction.

- [ ] **Step 7: Move commands to raw/local state callbacks**

Implement callbacks with these state rules:

- workspace get/set uses `entry.ui.setWorkspace`;
- dashboard disk JSON is normalized to `{ tasks, recentEdits }` and applied through `entry.runtime.update(state => applyDashboardData(state, data))`;
- permission response uses `beginInteractionSubmit`, invokes the command, locally resolves the raw interaction because the current protocol has no permission-resolved event, and clears submitting state in `finally`;
- ask submit/dismiss and plan review use the same duplicate-submit guard but wait for backend `ask_resolved`/`plan_resolved` before removing raw interaction;
- task action invokes the existing commands, then reloads typed dashboard data;
- undo invokes `cmd_undo_turn`, then calls `entry.runtime.update(state => removeTurnFromSession(state, turnId))` only after success;
- load-more uses `entry.state().turns[0]?.turnId` and relies on the `more_turns` event to prepend.

Use these exact interaction/undo controller shapes:

```ts
async function loadDashboardFromDisk(entry: SessionEntry) {
  try {
    const raw = await invoke<string>("cmd_get_dashboard_data", { seed: entry.state().seed });
    const parsed = JSON.parse(raw) as { tasks?: TaskInfo[]; recent_edits?: string[] };
    entry.runtime.update(state => applyDashboardData(state, {
      tasks: parsed.tasks ?? [],
      recentEdits: parsed.recent_edits ?? [],
    }));
  } catch (error) {
    console.error("loadDashboardFromDisk", error);
  }
}

async function loadWorkspace(entry: SessionEntry) {
  try {
    const workspace = await invoke<string>("cmd_get_workspace", { seed: entry.state().seed });
    entry.ui.setWorkspace(workspace);
    if (entry.state().seed === activeSeed()) setWorkspaceDraft(workspace);
  } catch (error) {
    console.error("loadWorkspace", error);
  }
}

async function submitTaskAction(
  action: "cancel" | "delete" | "ask",
  task: TaskInfo,
) {
  const entry = activeEntry();
  if (!entry) return;
  if (action === "ask") {
    await invoke("cmd_send_message", {
      seed: entry.state().seed,
      text: `Look at ${task.id}: ${task.subject}. Explain the implementation plan and current status in detail.`,
    });
    return;
  }
  const taskId = Number.parseInt(task.id.replace(/^T/, ""), 10);
  if (!Number.isFinite(taskId)) return;
  await invoke("cmd_task_action", { seed: entry.state().seed, action, taskId });
  await loadDashboardFromDisk(entry);
}

async function respondToPermission(
  item: Extract<PendingInteraction, { kind: "permission" }>,
  approved: boolean,
  trustFolder: boolean,
) {
  const entry = activeEntry();
  if (!entry || !entry.ui.beginInteractionSubmit(item.id)) return;
  try {
    await invoke("cmd_permission_response", {
      seed: entry.state().seed, toolCallId: item.id, approved, trustFolder,
    });
    entry.runtime.update(state => resolvePendingInteraction(
      state, item.id, approved ? "approved" : "rejected",
    ));
  } catch (error) {
    toastCtrl.push(String(error), "error");
  } finally {
    entry.ui.finishInteractionSubmit(item.id);
  }
}

async function submitAsk(
  item: Extract<PendingInteraction, { kind: "ask" }>,
  answers: AskAnswer[],
) {
  const entry = activeEntry();
  if (!entry || !entry.ui.beginInteractionSubmit(item.id)) return;
  try {
    await invoke("cmd_ask_response", { seed: entry.state().seed, askId: item.id, answers });
  } catch (error) {
    entry.ui.finishInteractionSubmit(item.id);
    toastCtrl.push(String(error), "error");
  }
}

async function dismissAsk(item: Extract<PendingInteraction, { kind: "ask" }>) {
  const entry = activeEntry();
  if (!entry || !entry.ui.beginInteractionSubmit(item.id)) return;
  try {
    await invoke("cmd_ask_dismiss", { seed: entry.state().seed, askId: item.id });
  } catch (error) {
    entry.ui.finishInteractionSubmit(item.id);
    toastCtrl.push(String(error), "error");
  }
}

async function respondToPlan(
  item: Extract<PendingInteraction, { kind: "plan" }>,
  approved: boolean,
  message?: string,
) {
  const entry = activeEntry();
  if (!entry || !entry.ui.beginInteractionSubmit(item.id)) return;
  try {
    await invoke("cmd_plan_review", {
      seed: entry.state().seed, callId: item.id, approved, message: message ?? null,
    });
  } catch (error) {
    entry.ui.finishInteractionSubmit(item.id);
    toastCtrl.push(String(error), "error");
  }
}

async function undoLastTurn() {
  const entry = activeEntry();
  const turnId = entry?.state().turns.at(-1)?.turnId;
  if (!entry || !turnId || isSessionStreaming(entry.state())) return;
  await invoke("cmd_undo_turn", { seed: entry.state().seed, turnId });
  entry.runtime.update(state => removeTurnFromSession(state, turnId));
}

async function newSession() {
  const seed = await invoke<string>("cmd_new_session");
  localStorage.removeItem(LS_KEY);
  await resumeSession(seed);
  const entry = activeEntry();
  const workspace = workspaceDraft();
  if (entry && workspace) {
    entry.ui.setWorkspace(workspace);
    await invoke("cmd_set_workspace", { seed: entry.state().seed, path: workspace });
  }
  await refreshSessions();
}

async function startNewSessionAndSend(text: string) {
  await newSession();
  const entry = activeEntry();
  if (entry) await invoke("cmd_send_message", { seed: entry.state().seed, text });
}

async function deleteSession(seed: string) {
  await invoke("cmd_delete_session", { seed });
  registry.remove(seed);
  if (activeSeed() === seed) {
    localStorage.removeItem(LS_KEY);
    setActiveSeed("");
    setHasChosenSession(false);
    setView("home");
  }
  await refreshSessions();
}
```

- [ ] **Step 8: Replace all view consumers and mount AppShell**

`SkillsView` reads `activeEntry()?.state().skills`. `ChatView` receives `rawSession={() => activeEntry()!.state()}`, `ui={activeEntry()!.ui}`, and the typed callbacks from Step 6.

Replace the outer shell with the complete active workspace switch:

```tsx
<AppShell
  sidebar={
    <TaskSidebar
      sessions={sessions()}
      activeSeed={activeSeed()}
      onNew={() => void newSession()}
      onOpen={seed => void resumeSession(seed)}
      onDelete={seed => void deleteSession(seed)}
      onSkills={() => setView("skills")}
      onSettings={() => setView("settings")}
    />
  }
  workspace={
    <Switch>
      <Match when={view() === "settings"}>
        <SettingsView
          lang={configLang}
          onLangChange={switchLang}
          theme={theme}
          onThemeChange={switchTheme}
          permissionLevel={permissionLevel()}
          onPermissionLevelChange={changePermissionLevel}
        />
      </Match>
      <Match when={view() === "skills"}>
        <SkillsView
          seed={activeSeed()}
          available={activeEntry()?.state().skills.available ?? []}
          active={activeEntry()?.state().skills.active ?? []}
          onActivate={async name => { await invoke("cmd_activate_skill", { seed: activeSeed(), name }); }}
          onUnload={async name => { await invoke("cmd_unload_skill", { seed: activeSeed(), name }); }}
          onReload={async () => { await invoke("cmd_reload_skills", { seed: activeSeed() }); }}
        />
      </Match>
      <Match when={view() === "home"}>
        <StartupView
          sessions={sessions()}
          onResume={resumeSession}
          onSend={startNewSessionAndSend}
          showHeatmap={false}
        />
      </Match>
      <Match when={view() === "chat"}>
        <Show when={hasChosenSession() && activeEntry()} keyed>
          {entry => <ChatView
            rawSession={entry.state}
            ui={entry.ui}
            onLoadMore={loadMoreTurns}
            onAskSubmit={submitAsk}
            onAskDismiss={dismissAsk}
            onPermissionRespond={respondToPermission}
            onPlanRespond={respondToPlan}
            onTaskAction={submitTaskAction}
            onUndo={undoLastTurn}
            permissionLevel={permissionLevel()}
            onPermissionLevelChange={changePermissionLevel}
            onChangeWorkspace={browseWorkspace}
          />}
        </Show>
      </Match>
    </Switch>
  }
/>
```

Delete the hidden `<aside class="sidebar frost-panel">`, sidebar width/version/changelog signals/effects, `formatDate`, `isActive`, and slash-command handler. Keep only navigation exposed by `TaskSidebar` and workspace controls exposed by `ThreadHeader`.

- [ ] **Step 9: Make cleanup frontend-only through the registry**

Replace cleanup with:

```ts
onCleanup(() => {
  registry.disposeView();
  sessionReplay.clear();
  unlistenTheme?.();
});
```

There must be no `cmd_close_session`, agent stop, or backend deletion in this path. Explicit delete still calls `cmd_delete_session` first, then `registry.remove(seed)`.

- [ ] **Step 10: Run App-adjacent tests and build**

```powershell
cd F:\DeepX-Fork\crates\deepx-tauri
pnpm exec vitest run src/store/sessionRegistry.test.ts src/store/sessionEventReducer.test.ts src/store/sessionEventRuntime.test.ts src/runtime/agentEventDispatcher.test.ts src/components/ChatView.interactions.test.tsx src/components/PlanReviewPanel.test.tsx src/components/shell/AppShell.test.tsx src/components/shell/TaskSidebar.test.tsx src/components/shell/ThreadHeader.test.tsx
pnpm build
```

Expected: all selected tests and `tsc --noEmit && vite build` pass. No `ChatStore` call-site error remains.

- [ ] **Step 11: Commit App and shell migration**

```powershell
cd F:\DeepX-Fork
git add crates/deepx-tauri/src/App.tsx crates/deepx-tauri/src/components/ChatView.tsx crates/deepx-tauri/src/components/ChatView.interactions.test.tsx crates/deepx-tauri/src/components/PlanReviewPanel.tsx crates/deepx-tauri/src/components/PlanReviewPanel.test.tsx crates/deepx-tauri/src/components/shell/ThreadHeader.tsx crates/deepx-tauri/src/components/shell/ThreadHeader.test.tsx crates/deepx-tauri/src/components/shell/AppShell.tsx crates/deepx-tauri/src/components/shell/AppShell.test.tsx crates/deepx-tauri/src/components/shell/TaskSidebar.tsx crates/deepx-tauri/src/components/shell/TaskSidebar.test.tsx crates/deepx-tauri/src/store/rawSession.ts crates/deepx-tauri/src/store/sessionEventReducer.ts crates/deepx-tauri/src/store/sessionEventReducer.test.ts crates/deepx-tauri/src/store/sessionEventRuntime.ts crates/deepx-tauri/src/store/sessionEventRuntime.test.ts crates/deepx-tauri/src/store/sessionRegistry.test.ts
git commit -m "refactor(tauri): use one frontend session pipeline"
git hash-object crates/deepx-tauri/src-tauri/Cargo.toml
```

Expected hash: `5b80caae02abced664f6801fbd98fb512e3d979e`.

---

### Task 8: Delete legacy files and add structural guards

**Files:**
- Create: `crates/deepx-tauri/src/legacyFrontendRemoval.test.ts`
- Modify: `crates/deepx-tauri/src/main.tsx`
- Delete: `crates/deepx-tauri/src/store/chat.ts`
- Delete: `crates/deepx-tauri/src/store/chat.ask.test.ts`
- Delete: `crates/deepx-tauri/src/store/permissionQueue.ts`
- Delete: `crates/deepx-tauri/src/store/permissionQueue.test.ts`
- Delete: `crates/deepx-tauri/src/store/environmentStore.ts`
- Delete: `crates/deepx-tauri/src/store/environmentStore.test.ts`
- Delete: `crates/deepx-tauri/src/store/orderedProgress.ts`
- Delete: `crates/deepx-tauri/src/store/orderedProgress.test.ts`
- Delete: `crates/deepx-tauri/src/components/AskDialog.tsx`
- Delete: `crates/deepx-tauri/src/components/AskDialog.test.tsx`
- Delete: `crates/deepx-tauri/src/components/AskForm.tsx`
- Delete: `crates/deepx-tauri/src/components/AskForm.test.tsx`
- Delete: `crates/deepx-tauri/src/components/ThinkingBlock.tsx`
- Delete: `crates/deepx-tauri/src/components/ToolRow.tsx`
- Delete: `crates/deepx-tauri/src/components/TokenChart.tsx`
- Delete: `crates/deepx-tauri/src/components/StockChart.tsx`
- Delete: `crates/deepx-tauri/src/components/SlashMenu.tsx`
- Delete: `crates/deepx-tauri/src/components/ChangelogModal.tsx`
- Delete: `crates/deepx-tauri/src/components/interactions/PlanApprovalPrompt.tsx`
- Delete: `crates/deepx-tauri/src/components/DiffBody.tsx.1781723177.1782263662`
- Delete: `crates/deepx-tauri/src/styles/sidebar.css`
- Delete: `crates/deepx-tauri/src/styles/slash-menu.css`
- Delete: `crates/deepx-tauri/src/styles/token-chart.css`
- Delete: `crates/deepx-tauri/src/styles/changelog.css`

**Interfaces:**
- Consumes: the migrated production graph from Task 7.
- Produces: a repository in which removed paths do not exist and production source cannot reintroduce old store/handler/shell patterns.

- [ ] **Step 1: Write a structural guard that fails while legacy code exists**

Create `legacyFrontendRemoval.test.ts`:

```ts
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { expect, it } from "vitest";

const root = resolve(process.cwd(), "src");
const removed = [
  "store/chat.ts", "store/permissionQueue.ts", "store/environmentStore.ts",
  "store/orderedProgress.ts", "components/AskDialog.tsx", "components/AskForm.tsx",
  "components/ThinkingBlock.tsx", "components/ToolRow.tsx", "components/TokenChart.tsx",
  "components/StockChart.tsx", "components/SlashMenu.tsx", "components/ChangelogModal.tsx",
  "components/interactions/PlanApprovalPrompt.tsx",
  "components/DiffBody.tsx.1781723177.1782263662", "styles/sidebar.css",
  "styles/slash-menu.css", "styles/token-chart.css", "styles/changelog.css",
];

function productionFiles(dir: string): string[] {
  return readdirSync(dir).flatMap(name => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return productionFiles(path);
    if (!/\.(ts|tsx)$/.test(name) || /\.test\.(ts|tsx)$/.test(name)) return [];
    return [path];
  });
}

it("contains no legacy frontend implementation", () => {
  expect(removed.filter(path => existsSync(join(root, path)))).toEqual([]);
  const source = productionFiles(root).map(path => readFileSync(path, "utf8")).join("\n");
  expect(source).not.toMatch(/createChatStore|chatStores|handleRoundDelta|handleToolCallPreview|handleRoundComplete|handleToolResults|handleExecProgress/);
  expect(source).not.toMatch(/\bpendingInteraction\b/);
  expect(source).not.toContain('<aside class="sidebar frost-panel">');
  expect(source).not.toMatch(/listen<Record<string, unknown>>|invoke<Record<string, unknown>\[\]>/);
});
```

- [ ] **Step 2: Run the guard and verify it lists existing legacy paths**

```powershell
cd F:\DeepX-Fork\crates\deepx-tauri
pnpm exec vitest run src/legacyFrontendRemoval.test.ts
```

Expected: FAIL with the current legacy file list.

- [ ] **Step 3: Delete the exact legacy inventory using apply_patch**

Delete every path listed in this task. Remove `sidebar.css`, `token-chart.css`, and any other deleted-style imports from `main.tsx`; remove `slash-menu.css` and `changelog.css` imports from `App.tsx`. Do not delete generated `CodeDaily.ts` or `CodeDeltaRecord.ts`, `AppShell`, current interaction prompts, or current conversation components.

- [ ] **Step 4: Run import and structural checks**

```powershell
cd F:\DeepX-Fork\crates\deepx-tauri
pnpm exec vitest run src/legacyFrontendRemoval.test.ts
rg -n "createChatStore|chatStores|permissionQueue|AskDialog|AskForm|ThinkingBlock|ToolRow|TokenChart|StockChart|SlashMenu|ChangelogModal|PlanApprovalPrompt|sidebar\.css|slash-menu\.css|token-chart\.css|changelog\.css" src
```

Expected: structural test passes. Run the `rg` command with `--glob '!legacyFrontendRemoval.test.ts'`; it exits 1 with no production matches.

- [ ] **Step 5: Run all frontend tests and build**

```powershell
pnpm test:run
pnpm build
```

Expected: zero failed files/tests and successful TypeScript/Vite build.

- [ ] **Step 6: Commit complete legacy deletion**

```powershell
cd F:\DeepX-Fork
git add -u -- crates/deepx-tauri/src
git add crates/deepx-tauri/src/legacyFrontendRemoval.test.ts crates/deepx-tauri/src/main.tsx
git commit -m "refactor(tauri): delete legacy frontend implementation"
git hash-object crates/deepx-tauri/src-tauri/Cargo.toml
```

Expected hash: `5b80caae02abced664f6801fbd98fb512e3d979e`.

---

### Task 9: Execute automated and manual acceptance gates

**Files:**
- Verify only. If a gate fails, add a focused regression test beside the failing unit before changing production code.

**Interfaces:**
- Consumes: Tasks 1-8.
- Produces: evidence that data mapping, streaming, replay, refresh, shell, deletion, and backend-scope constraints hold together.

- [ ] **Step 1: Run the complete automated gate from fresh commands**

```powershell
cd F:\DeepX-Fork\crates\deepx-tauri
pnpm test:run
pnpm build
cd F:\DeepX-Fork
cargo test -p deepx-proto -- export_bindings
cargo test -p deepx-tauri
cargo check -p deepx-tauri --tests
git diff --check
```

Expected: all frontend and Rust tests pass, build/check exit 0, and no whitespace errors exist. Existing unrelated Rust dead-code warnings are allowed; test or compile failures are not.

- [ ] **Step 2: Verify the one-pipeline structural contract**

```powershell
cd F:\DeepX-Fork
rg -n "createChatStore|chatStores|handleRoundDelta|handleToolCallPreview|handleRoundComplete|handleToolResults|handleExecProgress|\bpendingInteraction\b|listen<Record<string, unknown>>|invoke<Record<string, unknown>\[\]>" crates/deepx-tauri/src --glob '!legacyFrontendRemoval.test.ts'
rg -n "<aside class=\"sidebar|sidebar\.css|slash-menu\.css|token-chart\.css|changelog\.css" crates/deepx-tauri/src --glob '!legacyFrontendRemoval.test.ts'
git ls-files | rg "DiffBody\.tsx\.[0-9]|AskDialog|AskForm|ThinkingBlock|ToolRow|TokenChart|StockChart|SlashMenu|ChangelogModal|PlanApprovalPrompt|environmentStore|orderedProgress|permissionQueue|store/chat\.ts"
```

Expected: all three searches exit 1 with no matches.

- [ ] **Step 3: Verify protocol copies and user-owned backend state**

```powershell
Compare-Object (Get-Content crates\deepx-proto\bindings\Agent2Ui.ts) (Get-Content crates\deepx-tauri\src\lib\types\Agent2Ui.ts)
git hash-object crates/deepx-tauri/src-tauri/Cargo.toml
git status --short
git diff --name-only -- crates/deepx-msglp crates/deepx-tools crates/deepx-tauri/src-tauri
```

Expected: binding comparison has no output; Cargo hash is `5b80caae02abced664f6801fbd98fb512e3d979e`; the only uncommitted backend path is the pre-existing `crates/deepx-tauri/src-tauri/Cargo.toml`; no Ring-loop/tool/backend source changed.

- [ ] **Step 4: Run the Tauri live-stream and refresh smoke matrix**

```powershell
cd F:\DeepX-Fork\crates\deepx-tauri
pnpm tauri dev
```

Perform these checks in order:

1. Open an existing session and confirm historical prompts, reasoning, tools, stage answers, and final answers render.
2. Start a turn that streams reasoning, executes multiple tools with stdout/stderr, and produces a final answer.
3. Right-click refresh during reasoning; confirm the snapshot appears immediately and later deltas continue.
4. Refresh during tool execution; confirm tool identity, ordered chunks, stdout/stderr provenance, result, and following answer remain correct.
5. Refresh while permission, ask-user, and plan-review gates are pending; confirm the same gate returns and accepts exactly one response.
6. Refresh during final-answer streaming and after completion; confirm no blank, stale, duplicated, reordered, or delayed rows.
7. Switch sessions during and after turns; confirm workspaces, interactions, tasks, skills, usage, and transcripts do not leak across seeds.
8. Load older turns; confirm they prepend once and the scroll position remains stable.
9. Undo a turn; confirm it disappears from the authoritative transcript only after backend success.
10. Delete a session; confirm only that registry entry/listener/snapshot is removed.
11. Restart the application; confirm completed history restores and an active backend session can be resumed.
12. Confirm logs contain no `Unhandled Agent2Ui`, reducer crash loop, `cmd_close_session` on refresh, or backend agent kill caused by WebView cleanup.

- [ ] **Step 5: Record final verification and commit only if evidence documentation changed**

If no files changed during verification, do not create an empty commit. If a focused regression was required, rerun Steps 1-4 and commit only the test plus minimal fix with a message describing that defect.

## Completion Definition

This plan is complete only after Task 9 passes and:

- `createChatStore`, duplicate interaction queues, hidden sidebar, unreachable prototype components, obsolete styles/tests, and timestamped backup artifacts are absent;
- generated `Agent2Ui` is the only frontend agent-event interface after runtime validation;
- every event-bearing legacy responsibility is represented by raw state, a pure selector, local UI state, or an explicit dispatcher side effect;
- WebView refresh cannot clear the transcript, stop the backend session, lose pending gates, or detach future deltas;
- the user-owned `Cargo.toml` is unchanged and unstaged;
- no standalone backend, WebSocket, Ring-loop, or unrelated backend refactor entered this frontend plan.
