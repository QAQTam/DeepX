# DeepX Tauri Frontend Reliability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make thinking, tool, stage, and final-answer output render promptly; make WebView reload preserve and resume the visible conversation; and prevent high-frequency agent events from starving the Solid UI.

**Architecture:** Keep the current Ring loop and agent protocol unchanged. Repair Solid reactivity and Markdown latest-wins behavior, make the generated `Agent2Ui` union authoritative, then put the raw session reducer behind one animation-frame event runtime that writes a transient `sessionStorage` reload snapshot. A bounded replay cache in the `deepx-tauri` bridge retains only the current turn's non-delta lifecycle events so a new WebView listener can close the reload gap; the legacy `ChatStore` remains for composer/dashboard/interaction state but stops mirroring high-frequency transcript deltas.

**Tech Stack:** SolidJS 1.9, TypeScript 6, Vitest 4/jsdom, Tauri 2 JavaScript APIs, Rust `ts-rs` generated bindings, pnpm, PowerShell.

## Global Constraints

- Scope is Phase 0 frontend reliability only.
- Do not edit `crates/deepx-msglp/**`, `crates/deepx-tools/**`, Ring loop code, agent execution semantics, or introduce `deepx.exe`, WebSocket, or `ThreadManager`.
- Tauri Rust scope is limited to `agent_bridge/registry.rs`, `agent_bridge/commands/session.rs`, `agent_bridge/compile_checks.rs`, and `src/lib.rs` for the bounded replay cache and command registration. Do not edit any other Tauri Rust file.
- The existing modified `crates/deepx-tauri/src-tauri/Cargo.toml` is user-owned and must remain unstaged and byte-identical.
- The current working-file hash of that user-owned `Cargo.toml` is `5b80caae02abced664f6801fbd98fb512e3d979e`; verify the same hash at every commit boundary.
- A generated TypeScript binding update may touch `crates/deepx-proto/bindings/*.ts` and `crates/deepx-tauri/src/lib/types/*.ts`, but no Rust protocol source is changed in this phase.
- Use `sessionStorage`, not `localStorage`, for transient conversation reload snapshots.
- Add no runtime dependency and do not replace the existing Markdown, virtual-list, or state libraries.
- Keep the legacy `ChatStore`; remove only its high-frequency transcript mirroring. Broader store deletion belongs to a later frontend plan.
- Tests are written and observed failing before each implementation change.
- Stage files explicitly. Never run workspace-wide formatting or stage `crates/deepx-tauri/src-tauri/Cargo.toml`.

## Baseline Evidence

- Current HEAD before this plan: `67de4448f4a4f631ca3b49d671fc28d052d82384`.
- `pnpm exec vitest run src/components/conversation/TurnGroup.test.tsx` currently reports 1 failed / 1 passed; the completed assistant answer exists structurally but remains blank while Markdown highlighting initializes.
- `App.tsx` currently invokes `cmd_close_session` from `onCleanup`, so WebView reload kills backend agents.
- `Agent2Ui.ts` is stale: Rust contains `plan_submitted` and `plan_resolved`, while the copied TypeScript union does not.

## File Structure

- Modify `crates/deepx-tauri/src/components/conversation/TurnGroup.tsx` — reactive row visibility and live-round classification.
- Modify `crates/deepx-tauri/src/components/conversation/TurnGroup.test.tsx` — structural and live-update regression coverage independent of Shiki.
- Modify `crates/deepx-tauri/src/components/MarkdownBody.tsx` — latest-wins async render generation and synchronous plain-text fallback.
- Create `crates/deepx-tauri/src/components/MarkdownBody.test.tsx` — delayed-highlighter race regression.
- Regenerate `crates/deepx-proto/bindings/Agent2Ui.ts` and copy generated bindings to `crates/deepx-tauri/src/lib/types/`.
- Create `crates/deepx-tauri/src/lib/types/protocolBindings.test.ts` — copied binding drift check.
- Modify `crates/deepx-tauri/src/store/sessionEventReducer.ts` and `.test.ts` — typed plan lifecycle.
- Create `crates/deepx-tauri/src/store/sessionEventRuntime.ts` and `.test.ts` — frame-batched signal commits and transient reload snapshots.
- Modify `crates/deepx-tauri/src-tauri/src/agent_bridge/registry.rs` — bounded current-turn replay cache for non-delta lifecycle events.
- Modify `crates/deepx-tauri/src-tauri/src/agent_bridge/commands/session.rs`, `compile_checks.rs`, and `src/lib.rs` — read-only replay command and reachability.
- Create `crates/deepx-tauri/src/runtime/viewLifecycle.ts` and `.test.ts` — view-resource cleanup that cannot close backend sessions.
- Create `crates/deepx-tauri/src/runtime/sessionReplayBuffer.ts` and `.test.ts` — order bridge replay before live events and remove listener/snapshot overlap.
- Modify `crates/deepx-tauri/src/store/chat.ts` and `chat.ask.test.ts` — initialize the legacy store with its authoritative seed.
- Modify `crates/deepx-tauri/src/App.tsx` — integrate the raw event runtime, hydrate reload state, stop agent shutdown on view cleanup, and stop duplicate high-frequency transcript projection.

---

### Task 1: Make conversation rows reactive and classify the live round correctly

**Files:**
- Modify: `crates/deepx-tauri/src/components/conversation/TurnGroup.tsx:1-48`
- Modify: `crates/deepx-tauri/src/components/conversation/TurnGroup.test.tsx:1-72`

**Interfaces:**
- Consumes: `TurnViewModel`, `RoundViewModel`, `ProcessDisclosure`, `AssistantAnswer`.
- Produces: reactive `Show` predicates and `AssistantAnswer` props where only the last round of a running turn has `streaming=true`.

- [ ] **Step 1: Isolate TurnGroup tests from Markdown highlighting and add a live-update failure**

Add this mock after the imports in `TurnGroup.test.tsx`, retain the two existing tests, and append the new test:

```tsx
import { createSignal } from "solid-js";

vi.mock("../MarkdownBody", () => ({
  default: (props: { content: string; final?: boolean }) => (
    <div data-markdown-final={props.final ? "true" : "false"}>{props.content}</div>
  ),
}));

it("reacts when process items and the active answer arrive after mount", async () => {
  const host = document.createElement("div");
  const [turn, setTurn] = createSignal<TurnViewModel>({
    turnId: "turn-live",
    userPrompt: "开始",
    status: "running",
    rounds: [{ roundNum: 0, isFinal: false, processItems: [] }],
    interactions: [],
  });
  const dispose = render(() => <TurnGroup turn={turn()} />, host);

  expect(host.querySelector('[data-part="process"]')).toBeNull();

  setTurn(current => ({
    ...current,
    rounds: [{
      roundNum: 0,
      isFinal: false,
      processItems: [{ kind: "reasoning", id: "r-live", content: "正在分析" }],
      answer: "输出中",
    }],
  }));

  await vi.waitFor(() => {
    expect(host.querySelector('[data-part="process"]')?.textContent).toContain("正在分析");
    expect(host.querySelector('[data-part="assistant-answer"]')?.textContent).toContain("输出中");
  });
  expect(host.querySelector('[data-stage="true"]')).toBeNull();
  expect(host.querySelector('[data-markdown-final="false"]')).not.toBeNull();

  setTurn(current => ({
    ...current,
    status: "completed",
    rounds: current.rounds.map(round => ({ ...round, isFinal: true })),
  }));

  await vi.waitFor(() =>
    expect(host.querySelector('[data-markdown-final="true"]')).not.toBeNull(),
  );
  dispose();
});
```

- [ ] **Step 2: Run the focused test and verify the new assertion fails**

Run:

```powershell
cd F:\DeepX-Fork\crates\deepx-tauri
pnpm exec vitest run src/components/conversation/TurnGroup.test.tsx
```

Expected: the new test fails because `hasItems` and `defaultOpen` were captured as non-reactive constants, or because the live answer is incorrectly marked final/stage.

- [ ] **Step 3: Replace captured values with accessors and derive live status from turn position**

Replace the `Index` callback body in `TurnGroup.tsx` with:

```tsx
<Index each={props.turn.rounds}>
  {(round, index) => {
    const hasItems = () => round().processItems.length > 0;
    const isLiveRound = () =>
      status() === "running" && index === props.turn.rounds.length - 1;
    const isStage = () => !round().isFinal && !isLiveRound();
    const defaultOpen = () =>
      !round().answer || isLiveRound() || status() === "waiting";

    return (
      <>
        <Show when={hasItems()}>
          <div data-part="process">
            <ProcessDisclosure
              status={status()}
              defaultOpen={defaultOpen()}
              tokensPerSec={
                round().isFinal && status() === "completed"
                  ? props.turn.tokensPerSec
                  : undefined
              }
            >
              <ProcessTimeline items={round().processItems} />
            </ProcessDisclosure>
          </div>
        </Show>
        <Show when={round().answer}>
          {(answer) => (
            <AssistantAnswer
              markdown={answer()}
              stage={isStage()}
              streaming={isLiveRound()}
            />
          )}
        </Show>
      </>
    );
  }}
</Index>
```

Delete the `[TURN_GROUP]` `console.log`.

- [ ] **Step 4: Run the focused test and verify all TurnGroup tests pass**

Run:

```powershell
pnpm exec vitest run src/components/conversation/TurnGroup.test.tsx
```

Expected: `3 passed`, with no `[TURN_GROUP]` console output.

- [ ] **Step 5: Commit the reactive rendering fix**

```powershell
git add crates/deepx-tauri/src/components/conversation/TurnGroup.tsx crates/deepx-tauri/src/components/conversation/TurnGroup.test.tsx
git commit -m "fix(tauri): keep conversation rows reactive"
```

---

### Task 2: Make Markdown rendering synchronous-first and latest-wins

**Files:**
- Modify: `crates/deepx-tauri/src/components/MarkdownBody.tsx:1-290`
- Create: `crates/deepx-tauri/src/components/MarkdownBody.test.tsx`

**Interfaces:**
- Consumes: `content`, `final`, shared Shiki highlighter promise.
- Produces: immediate readable text plus highlighted HTML only when the completing async render still matches the newest content generation.

- [ ] **Step 1: Write a delayed-highlighter regression test**

Create `MarkdownBody.test.tsx`:

```tsx
// @vitest-environment jsdom

import { createSignal } from "solid-js";
import { render } from "solid-js/web";
import { expect, it, vi } from "vitest";

const shikiState = vi.hoisted(() => {
  let resolve!: (value: { codeToHtml: (text: string) => string }) => void;
  const promise = new Promise<{ codeToHtml: (text: string) => string }>(r => {
    resolve = r;
  });
  return { promise, resolve };
});

vi.mock("shiki", () => ({
  createHighlighter: vi.fn(() => shikiState.promise),
  createOnigurumaEngine: vi.fn(() => ({})),
}));

import MarkdownBody from "./MarkdownBody";

it("shows final text immediately and ignores an older async render", async () => {
  const host = document.createElement("div");
  const [content, setContent] = createSignal("old answer");
  const dispose = render(
    () => <MarkdownBody content={content()} final={true} />,
    host,
  );

  expect(host.textContent).toContain("old answer");
  setContent("new answer");
  expect(host.textContent).toContain("new answer");

  shikiState.resolve({
    codeToHtml: text => `<pre><code>${text}</code></pre>`,
  });

  await vi.waitFor(() => expect(host.textContent).toContain("new answer"));
  expect(host.textContent).not.toContain("old answer");
  dispose();
});
```

- [ ] **Step 2: Run the focused test and verify immediate final text is missing**

Run:

```powershell
pnpm exec vitest run src/components/MarkdownBody.test.tsx
```

Expected: FAIL at the first immediate-text assertion because the current final path awaits Shiki before writing DOM.

- [ ] **Step 3: Add render generations, cleanup invalidation, fallback text, and guarded awaits**

Change the Solid import to:

```ts
import { on, createEffect, onCleanup } from "solid-js";
```

Replace the component body with:

```tsx
export default function MarkdownBody(props: MarkdownBodyProps) {
  let container!: HTMLDivElement;
  let prevBlocks: MarkdownBlock[] = [];
  let renderGeneration = 0;
  let disposed = false;

  onCleanup(() => {
    disposed = true;
    renderGeneration += 1;
  });

  createEffect(on(() => [props.content, props.final] as const, async ([text, final]) => {
    const generation = ++renderGeneration;
    const isStale = () => disposed || generation !== renderGeneration;

    if (!text) {
      container.innerHTML = "";
      container.classList.remove("final");
      prevBlocks = [];
      return;
    }

    const blocks = projectBlocks(text, !!final, prevBlocks);

    if (final) {
      container.textContent = text;
      container.classList.remove("final");
      let hi: Awaited<ReturnType<typeof getHi>>;
      try {
        hi = await getHi();
      } catch {
        if (!isStale()) prevBlocks = blocks;
        return;
      }
      if (isStale() || !hi) return;
      if (!blocks[0]!.html) {
        blocks[0]!.html = renderBlockHTML(blocks[0]!.raw, hi);
      }
      if (isStale()) return;
      container.innerHTML = "";
      container.appendChild(createStableEl(blocks[0]!));
      container.classList.add("final");
      prevBlocks = blocks;
      return;
    }

    container.classList.remove("final");
    const needsRender = blocks.some(block => block.stable && !block.html);
    if (needsRender) {
      container.textContent = paceText(text);
      let hi: Awaited<ReturnType<typeof getHi>>;
      try {
        hi = await getHi();
      } catch {
        if (!isStale()) prevBlocks = blocks;
        return;
      }
      if (isStale() || !hi) return;
      for (const block of blocks) {
        if (block.stable && !block.html) {
          block.html = renderBlockHTML(block.raw, hi);
        }
      }
    }

    if (isStale()) return;
    patchDOM(container, blocks);
    prevBlocks = blocks;
  }));

  return <div ref={container} class={props.class} />;
}
```

- [ ] **Step 4: Run Markdown and TurnGroup regression tests**

Run:

```powershell
pnpm exec vitest run src/components/MarkdownBody.test.tsx src/components/conversation/TurnGroup.test.tsx
```

Expected: both files pass; a final answer is readable before Shiki resolves, and an old promise cannot overwrite new content.

- [ ] **Step 5: Commit the Markdown race fix**

```powershell
git add crates/deepx-tauri/src/components/MarkdownBody.tsx crates/deepx-tauri/src/components/MarkdownBody.test.tsx
git commit -m "fix(tauri): make markdown rendering latest-wins"
```

---

### Task 3: Restore generated protocol exhaustiveness for plan events

**Files:**
- Create: `crates/deepx-tauri/src/lib/types/protocolBindings.test.ts`
- Regenerate: `crates/deepx-proto/bindings/*.ts`
- Regenerate: `crates/deepx-tauri/src/lib/types/*.ts`
- Modify: `crates/deepx-tauri/src/store/sessionEventReducer.ts:120-310`
- Modify: `crates/deepx-tauri/src/store/sessionEventReducer.test.ts:1-88`

**Interfaces:**
- Consumes: Rust `Agent2Ui::{PlanSubmitted, PlanResolved}` generated through `ts-rs`.
- Produces: matching TypeScript variants, raw pending-interaction transitions, and duplicate-free tail reconciliation for reload resync.

- [ ] **Step 1: Add a binding drift test that fails on the current stale copy**

Create `protocolBindings.test.ts`:

```ts
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { expect, it } from "vitest";

it("keeps the copied Agent2Ui binding equal to the generated Rust binding", () => {
  const generated = readFileSync(
    resolve(process.cwd(), "../deepx-proto/bindings/Agent2Ui.ts"),
    "utf8",
  );
  const copied = readFileSync(
    resolve(process.cwd(), "src/lib/types/Agent2Ui.ts"),
    "utf8",
  );

  expect(generated).toContain('"type": "plan_submitted"');
  expect(generated).toContain('"type": "plan_resolved"');
  expect(copied).toBe(generated);
});
```

- [ ] **Step 2: Run the binding test and verify it fails**

Run:

```powershell
pnpm exec vitest run src/lib/types/protocolBindings.test.ts
```

Expected: FAIL because the committed generated binding does not contain `plan_submitted`.

- [ ] **Step 3: Regenerate Rust bindings and copy them mechanically**

Run from the repository root:

```powershell
cd F:\DeepX-Fork
cargo test -p deepx-proto -- export_bindings
Copy-Item -Force crates\deepx-proto\bindings\*.ts crates\deepx-tauri\src\lib\types\
```

Expected: `Agent2Ui.ts` now contains both plan variants. Inspect `git diff --stat` and retain only generator-produced changes.

- [ ] **Step 4: Add a failing reducer lifecycle test**

Append to `sessionEventReducer.test.ts`:

```ts
it("tracks plan review as a waiting interaction and resolves it", () => {
  let state = createRawSessionState("seed-a");
  state = reduceAgentEvent(state, {
    type: "turn_start", turn_id: "t-plan", user_text: "plan",
  }, 100);
  state = reduceAgentEvent(state, {
    type: "plan_submitted", call_id: "plan-1", plan_content: "# Plan",
  }, 110);

  expect(state.turns[0].status).toBe("waiting");
  expect(state.pendingInteraction).toEqual({ kind: "plan", id: "plan-1" });

  state = reduceAgentEvent(state, {
    type: "plan_resolved", call_id: "plan-1", approved: true,
  }, 120);

  expect(state.turns[0].status).toBe("running");
  expect(state.pendingInteraction).toBeNull();
  expect(state.turns[0].interactions.at(-1)).toMatchObject({
    id: "plan-1", kind: "plan", resolution: "approved",
  });
});

it("does not duplicate consecutive notices when lifecycle events are replayed", () => {
  let state = createRawSessionState("seed-a");
  const event = { type: "error" as const, message: "agent exited" };
  state = reduceAgentEvent(state, event, 100);
  state = reduceAgentEvent(state, event, 110);
  expect(state.notices).toHaveLength(1);
});
```

- [ ] **Step 5: Run the reducer test and verify the plan event reaches `assertNever`**

Run:

```powershell
pnpm exec vitest run src/store/sessionEventReducer.test.ts
```

Expected: the plan test fails with `Unhandled Agent2Ui event`; the replay test exposes duplicate error notices.

- [ ] **Step 6: Implement typed plan state transitions**

Add these cases before `compact_start` in `reduceAgentEvent`:

```ts
case "plan_submitted": {
  const turnId = lastTurnId(state);
  const next = {
    ...state,
    pendingInteraction: { kind: "plan" as const, id: event.call_id },
  };
  return turnId
    ? updateTurn(next, turnId, turn => ({ ...turn, status: "waiting" }))
    : next;
}
case "plan_resolved":
  return resolvePendingInteraction(
    state,
    event.call_id,
    event.approved ? "approved" : "rejected",
    now,
  );
```

Do not add these variants to the no-op group. Do not change `more_turns`: historical `TurnData` does not encode active/completed lifecycle state and must not be used as reload resynchronization.

Add a helper that appends a notice only when the immediately preceding notice does not have the same level and message. Use it in both `error` and `ask_rejected`; keep the existing failed-turn and interaction transitions unchanged. This makes lifecycle replay idempotent without suppressing the same message later after another notice.

- [ ] **Step 7: Run protocol and reducer tests plus TypeScript build**

Run:

```powershell
cd F:\DeepX-Fork\crates\deepx-tauri
pnpm exec vitest run src/lib/types/protocolBindings.test.ts src/store/sessionEventReducer.test.ts
pnpm build
```

Expected: both test files pass and `tsc --noEmit && vite build` exits 0.

- [ ] **Step 8: Commit protocol synchronization explicitly**

```powershell
git add crates/deepx-proto/bindings crates/deepx-tauri/src/lib/types crates/deepx-tauri/src/store/sessionEventReducer.ts crates/deepx-tauri/src/store/sessionEventReducer.test.ts
git commit -m "fix(tauri): sync plan protocol bindings"
```

---

### Task 4: Batch raw session commits and persist a transient reload snapshot

**Files:**
- Create: `crates/deepx-tauri/src/store/sessionEventRuntime.ts`
- Create: `crates/deepx-tauri/src/store/sessionEventRuntime.test.ts`

**Interfaces:**
- Consumes: `RawSessionState`, `Agent2Ui`, `reduceAgentEvent`, browser-compatible `Storage`.
- Produces:
  - `createSessionEventRuntime(options): SessionEventRuntime`
  - `loadReloadSnapshot(storage, seed): RawSessionState | undefined`
  - `removeReloadSnapshot(storage, seed): void`
  - `SessionEventRuntime.push`, `.update`, `.flush`, `.dispose`, `.current`.

- [ ] **Step 1: Write batching, terminal flush, persistence, and invalid-snapshot tests**

Create `sessionEventRuntime.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { createRawSessionState } from "./sessionEventReducer";
import {
  createSessionEventRuntime,
  loadReloadSnapshot,
  type ReloadStorage,
} from "./sessionEventRuntime";

class MemoryStorage implements ReloadStorage {
  private values = new Map<string, string>();
  getItem(key: string) { return this.values.get(key) ?? null; }
  setItem(key: string, value: string) { this.values.set(key, value); }
  removeItem(key: string) { this.values.delete(key); }
}

describe("sessionEventRuntime", () => {
  it("commits streaming deltas once per frame and terminal events immediately", () => {
    const storage = new MemoryStorage();
    const commits: string[] = [];
    const scheduled: Array<() => void> = [];
    const runtime = createSessionEventRuntime({
      initialState: createRawSessionState("seed-a"),
      commit: state => commits.push(state.turns[0]?.rounds[0]?.answer ?? ""),
      storage,
      schedule: flush => scheduled.push(flush),
      now: () => 100,
    });

    runtime.push({ type: "turn_start", turn_id: "t1", user_text: "go" });
    expect(commits).toHaveLength(1);

    runtime.push({ type: "round_delta", turn_id: "t1", round_num: 0, kind: "answering", delta: "A" });
    runtime.push({ type: "round_delta", turn_id: "t1", round_num: 0, kind: "answering", delta: "B" });
    expect(commits).toHaveLength(1);
    expect(scheduled).toHaveLength(1);

    scheduled[0]!();
    expect(commits.at(-1)).toBe("AB");

    runtime.push({ type: "turn_end", turn_id: "t1" });
    expect(runtime.current().turns[0].status).toBe("completed");
    expect(commits).toHaveLength(3);
  });

  it("flushes on dispose and restores the last twenty turns", () => {
    const storage = new MemoryStorage();
    const state = createRawSessionState("seed-a");
    state.turns = Array.from({ length: 25 }, (_, index) => ({
      turnId: `t${index}`,
      userText: `${index}`,
      status: "completed" as const,
      rounds: [],
      interactions: [],
    }));
    const runtime = createSessionEventRuntime({
      initialState: state,
      commit: () => {},
      storage,
      schedule: () => {},
    });

    runtime.dispose();
    const restored = loadReloadSnapshot(storage, "seed-a");
    expect(restored?.turns).toHaveLength(20);
    expect(restored?.turns[0].turnId).toBe("t5");
  });

  it("rejects corrupt or wrong-seed snapshots", () => {
    const storage = new MemoryStorage();
    storage.setItem("deepx:reload:v1:seed-a", "not-json");
    expect(loadReloadSnapshot(storage, "seed-a")).toBeUndefined();

    storage.setItem("deepx:reload:v1:seed-a", JSON.stringify({
      version: 1,
      state: { ...createRawSessionState("seed-b"), seed: "seed-b" },
    }));
    expect(loadReloadSnapshot(storage, "seed-a")).toBeUndefined();
  });
});
```

- [ ] **Step 2: Run the focused test and verify the module is missing**

Run:

```powershell
pnpm exec vitest run src/store/sessionEventRuntime.test.ts
```

Expected: FAIL because `sessionEventRuntime.ts` does not exist.

- [ ] **Step 3: Implement the event runtime and bounded transient snapshot**

Create `sessionEventRuntime.ts`:

```ts
import type { Agent2Ui } from "../lib/types";
import type { RawSessionState } from "./rawSession";
import { reduceAgentEvent } from "./sessionEventReducer";

export type ReloadStorage = Pick<Storage, "getItem" | "setItem" | "removeItem">;
export type ScheduleFlush = (flush: () => void) => void;

const SNAPSHOT_VERSION = 1;
const SNAPSHOT_PREFIX = "deepx:reload:v1:";
const MAX_RELOAD_TURNS = 20;
const MAX_PROGRESS_CHUNKS = 200;

const IMMEDIATE_EVENT_TYPES = new Set<Agent2Ui["type"]>([
  "turn_start",
  "turn_end",
  "round_complete",
  "tool_results",
  "session_restored",
  "more_turns",
  "session_created",
  "error",
  "permission_request",
  "ask_user",
  "ask_resolved",
  "ask_rejected",
  "plan_submitted",
  "plan_resolved",
  "compact_start",
  "compact_end",
  "cancelled",
  "done",
]);

function reloadKey(seed: string): string {
  return `${SNAPSHOT_PREFIX}${seed}`;
}

function compactReloadState(state: RawSessionState): RawSessionState {
  return {
    ...state,
    turns: state.turns.slice(-MAX_RELOAD_TURNS).map(turn => ({
      ...turn,
      rounds: turn.rounds.map(round => ({
        ...round,
        progress: Object.fromEntries(
          Object.entries(round.progress).map(([id, progress]) => [
            id,
            { chunks: progress.chunks.slice(-MAX_PROGRESS_CHUNKS) },
          ]),
        ),
      })),
    })),
  };
}

function saveReloadSnapshot(storage: ReloadStorage, state: RawSessionState): void {
  try {
    storage.setItem(reloadKey(state.seed), JSON.stringify({
      version: SNAPSHOT_VERSION,
      state: compactReloadState(state),
    }));
  } catch (error) {
    console.warn("[reload-snapshot] save failed", error);
  }
}

export function loadReloadSnapshot(
  storage: ReloadStorage,
  seed: string,
): RawSessionState | undefined {
  try {
    const raw = storage.getItem(reloadKey(seed));
    if (!raw) return undefined;
    const parsed = JSON.parse(raw) as { version?: number; state?: RawSessionState };
    if (
      parsed.version !== SNAPSHOT_VERSION ||
      parsed.state?.seed !== seed ||
      !Array.isArray(parsed.state.turns)
    ) {
      storage.removeItem(reloadKey(seed));
      return undefined;
    }
    return parsed.state;
  } catch {
    storage.removeItem(reloadKey(seed));
    return undefined;
  }
}

export function removeReloadSnapshot(storage: ReloadStorage, seed: string): void {
  storage.removeItem(reloadKey(seed));
}

export interface SessionEventRuntime {
  push(event: Agent2Ui): void;
  update(update: (state: RawSessionState) => RawSessionState): void;
  flush(): void;
  dispose(): void;
  current(): RawSessionState;
}

export function createSessionEventRuntime(options: {
  initialState: RawSessionState;
  commit: (state: RawSessionState) => void;
  storage: ReloadStorage;
  schedule?: ScheduleFlush;
  now?: () => number;
}): SessionEventRuntime {
  let state = options.initialState;
  let scheduled = false;
  let disposed = false;
  const now = options.now ?? Date.now;
  const schedule = options.schedule ?? ((flush: () => void) => {
    requestAnimationFrame(flush);
  });

  const commitAndPersist = () => {
    options.commit(state);
    saveReloadSnapshot(options.storage, state);
  };

  const flush = () => {
    if (disposed) return;
    scheduled = false;
    commitAndPersist();
  };

  const scheduleCommit = () => {
    if (scheduled || disposed) return;
    scheduled = true;
    schedule(() => {
      if (disposed || !scheduled) return;
      flush();
    });
  };

  return {
    push(event) {
      if (disposed) return;
      state = reduceAgentEvent(state, event, now());
      if (IMMEDIATE_EVENT_TYPES.has(event.type)) flush();
      else scheduleCommit();
    },
    update(update) {
      if (disposed) return;
      state = update(state);
      flush();
    },
    flush,
    dispose() {
      if (disposed) return;
      flush();
      disposed = true;
    },
    current() {
      return state;
    },
  };
}
```

- [ ] **Step 4: Run runtime and reducer tests**

Run:

```powershell
pnpm exec vitest run src/store/sessionEventRuntime.test.ts src/store/sessionEventReducer.test.ts
```

Expected: both files pass. The two deltas produce one scheduled signal commit, while `turn_end` commits synchronously.

- [ ] **Step 5: Commit the isolated event runtime**

```powershell
git add crates/deepx-tauri/src/store/sessionEventRuntime.ts crates/deepx-tauri/src/store/sessionEventRuntime.test.ts
git commit -m "perf(tauri): batch raw session event commits"
```

---

### Task 5: Add a bounded WebView listener-gap replay cache in the Tauri bridge

**Files:**
- Modify: `crates/deepx-tauri/src-tauri/src/agent_bridge/registry.rs`
- Modify: `crates/deepx-tauri/src-tauri/src/agent_bridge/commands/session.rs`
- Modify: `crates/deepx-tauri/src-tauri/src/agent_bridge/compile_checks.rs`
- Modify: `crates/deepx-tauri/src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: already-decoded `serde_json::Value` events in the Tauri stdout bridge.
- Produces: `cmd_replay_session_events(seed) -> Vec<serde_json::Value>` containing only the current turn's bounded, non-delta lifecycle events.
- Invariant: no Ring loop, `Ui2Agent`, `Agent2Ui`, database, subprocess ownership, or transport change.

- [ ] **Step 1: Add failing pure replay-cache tests**

In `registry.rs`, extend the existing `tests` module with tests against a private `record_replay_event` helper:

```rust
#[test]
fn replay_cache_keeps_lifecycle_events_and_drops_deltas() {
    let cache = new_replay_cache();
    record_replay_event(&cache, &serde_json::json!({
        "type": "turn_start", "turn_id": "t1", "user_text": "reload"
    }));
    record_replay_event(&cache, &serde_json::json!({
        "type": "round_delta", "turn_id": "t1", "delta": "partial"
    }));
    record_replay_event(&cache, &serde_json::json!({
        "type": "round_complete", "turn_id": "t1", "round_num": 0,
        "thinking": null, "answer": "complete", "tool_calls": []
    }));

    let events = cache.lock().unwrap();
    let types: Vec<_> = events.iter()
        .filter_map(|event| event.get("type").and_then(|value| value.as_str()))
        .collect();
    assert_eq!(types, vec!["turn_start", "round_complete"]);
}

#[test]
fn a_new_turn_discards_the_previous_turn_replay() {
    let cache = new_replay_cache();
    record_replay_event(&cache, &serde_json::json!({ "type": "done" }));
    record_replay_event(&cache, &serde_json::json!({
        "type": "turn_start", "turn_id": "t2", "user_text": "next"
    }));

    let events = cache.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["type"], "turn_start");
}
```

Update the existing `test_agent_instance_fields_accessible` constructor to initialize the new cache field.

- [ ] **Step 2: Compile the focused Rust tests and verify they fail**

Build the frontend once because the Tauri build script expects `dist/`, then compile the Tauri test targets:

```powershell
cd F:\DeepX-Fork\crates\deepx-tauri
pnpm build
cd src-tauri
cargo check -p deepx-tauri --tests
```

Expected: FAIL because `new_replay_cache` and `record_replay_event` do not exist. Use `cargo check --tests`, matching the crate's existing compile-check policy; do not claim these unit functions executed on Windows where the Tauri runtime DLL is unavailable to test binaries.

- [ ] **Step 3: Implement the bounded current-turn cache**

In `registry.rs`, change the collection import to `HashMap, VecDeque` and add:

```rust
const REPLAY_EVENT_LIMIT: usize = 128;
type ReplayCache = Arc<Mutex<VecDeque<serde_json::Value>>>;

fn new_replay_cache() -> ReplayCache {
    Arc::new(Mutex::new(VecDeque::new()))
}

fn is_replayable_event(event_type: &str) -> bool {
    matches!(
        event_type,
        "turn_start"
            | "round_complete"
            | "tool_results"
            | "turn_end"
            | "done"
            | "cancelled"
            | "error"
            | "permission_request"
            | "ask_user"
            | "ask_resolved"
            | "ask_rejected"
            | "plan_submitted"
            | "plan_resolved"
    )
}

fn record_replay_event(cache: &ReplayCache, payload: &serde_json::Value) {
    let Some(event_type) = payload.get("type").and_then(|value| value.as_str()) else {
        return;
    };
    if !is_replayable_event(event_type) {
        return;
    }
    let Ok(mut events) = cache.lock() else { return };
    if event_type == "turn_start" {
        events.clear();
    }
    while events.len() >= REPLAY_EVENT_LIMIT {
        events.pop_front();
    }
    events.push_back(payload.clone());
}
```

Add `replay_events: ReplayCache` to `AgentInstance`. Create it before the stdout reader thread, clone it into that thread, call `record_replay_event(&replay_for_reader, &payload)` after parsing, and retain the original cache in the constructed `AgentInstance`. Also record the synthetic `Agent2Ui::Error` payload produced when the stdout reader exits before attempting its final emits, so a WebView gap cannot hide process death.

Do not cache `round_delta`, tool previews, progress chunks, compact deltas, restore/history events, or session metadata. `sessionStorage` retains the last visible deltas; the bridge cache only protects lifecycle transitions that cannot be reconstructed from `TurnData`.

- [ ] **Step 4: Keep the stdout reader alive across a detached WebView**

Replace the current emit-error `break`:

```rust
if let Err(error) = app_handle.emit(&event_label, &payload) {
    log::warn!(
        "[REGISTRY] WebView emit failed for {seed_owned}; retaining agent stdout: {error}"
    );
}
```

The stdout reader owns the agent protocol stream and must not terminate merely because the WebView listener is being replaced during refresh. Continue reading and recording replayable events; native process shutdown remains unchanged.

- [ ] **Step 5: Expose a read-only replay command**

Add this method to `AgentRegistry`:

```rust
pub fn replay_events(&self, seed: &str) -> Result<Vec<serde_json::Value>, String> {
    let instance = self.instances.get(seed)
        .ok_or_else(|| format!("No running agent for seed {seed}"))?;
    let events = instance.replay_events.lock()
        .map_err(|error| format!("replay cache lock: {error}"))?;
    Ok(events.iter().cloned().collect())
}
```

Add to `commands/session.rs`:

```rust
#[tauri::command]
pub fn cmd_replay_session_events(seed: String) -> Result<Vec<serde_json::Value>, String> {
    let registry = AgentRegistry::get()
        .lock()
        .map_err(|error| format!("registry lock: {error}"))?;
    registry.replay_events(&seed)
}
```

Register `cmd_replay_session_events` next to `cmd_resume_session` in `src/lib.rs`, and add this reachability check in `compile_checks.rs`:

```rust
let _ = agent_bridge::cmd_replay_session_events as fn(_) -> _;
```

- [ ] **Step 6: Verify the isolated bridge change**

```powershell
cd F:\DeepX-Fork
cargo check -p deepx-tauri --tests
git diff --check -- crates/deepx-tauri/src-tauri/src
git hash-object crates/deepx-tauri/src-tauri/Cargo.toml
```

Expected: replay assertions and command reachability compile; diff check is clean; `Cargo.toml` remains `5b80caae02abced664f6801fbd98fb512e3d979e`. Runtime behavior is covered by the Task 6 TypeScript coordinator tests and the Task 7 Tauri smoke test.

- [ ] **Step 7: Commit only the bounded Tauri bridge files**

```powershell
git add crates/deepx-tauri/src-tauri/src/agent_bridge/registry.rs crates/deepx-tauri/src-tauri/src/agent_bridge/commands/session.rs crates/deepx-tauri/src-tauri/src/agent_bridge/compile_checks.rs crates/deepx-tauri/src-tauri/src/lib.rs
git commit -m "fix(tauri): replay lifecycle events after webview reload"
```

---

### Task 6: Integrate reload-safe view lifecycle and listener replay

**Files:**
- Create: `crates/deepx-tauri/src/runtime/viewLifecycle.ts`
- Create: `crates/deepx-tauri/src/runtime/viewLifecycle.test.ts`
- Create: `crates/deepx-tauri/src/runtime/sessionReplayBuffer.ts`
- Create: `crates/deepx-tauri/src/runtime/sessionReplayBuffer.test.ts`
- Modify: `crates/deepx-tauri/src/store/chat.ts:31-35`
- Modify: `crates/deepx-tauri/src/store/chat.ask.test.ts:1-95`
- Modify: `crates/deepx-tauri/src/App.tsx:1-528`

**Interfaces:**
- Consumes: `SessionEventRuntime`, `sessionStorage`, `cmd_replay_session_events`, Tauri listener unlisten functions, existing `ChatStore` lifecycle methods.
- Produces: reload hydration followed by ordered lifecycle replay, live-event buffering during replay, cleanup that only disposes frontend resources, authoritative initial session seed, and raw-only high-frequency transcript updates.

- [ ] **Step 1: Add failing cleanup and initial-seed tests**

Create `runtime/viewLifecycle.test.ts`:

```ts
import { expect, it, vi } from "vitest";
import { cleanupViewResources } from "./viewLifecycle";

it("flushes runtimes before removing listeners and never receives a backend closer", () => {
  const order: string[] = [];
  const runtime = { dispose: vi.fn(() => order.push("runtime")) };
  const listener = vi.fn(() => order.push("listener"));
  const theme = vi.fn(() => order.push("theme"));

  cleanupViewResources([runtime], [listener], theme);

  expect(order).toEqual(["runtime", "listener", "theme"]);
});
```

Create `runtime/sessionReplayBuffer.test.ts`:

```ts
import { expect, it } from "vitest";
import { createSessionReplayBuffer } from "./sessionReplayBuffer";

it("applies replay before live events and removes exact overlap", () => {
  const buffer = createSessionReplayBuffer();
  const applied: string[] = [];
  const apply = (event: Record<string, unknown>) => applied.push(String(event.id));
  const overlap = { type: "round_complete", id: "complete" };

  buffer.begin("seed-a");
  buffer.handleLive("seed-a", overlap, apply);
  buffer.handleLive("seed-a", { type: "turn_end", id: "end" }, apply);
  expect(applied).toEqual([]);

  buffer.complete("seed-a", [
    { type: "turn_start", id: "start" },
    overlap,
  ], apply);

  expect(applied).toEqual(["start", "complete", "end"]);
});

it("drains buffered live events when replay is unavailable", () => {
  const buffer = createSessionReplayBuffer();
  const applied: string[] = [];
  buffer.begin("seed-a");
  buffer.handleLive("seed-a", { type: "turn_end", id: "end" }, event => {
    applied.push(String(event.id));
  });
  buffer.abort("seed-a", event => applied.push(String(event.id)));
  expect(applied).toEqual(["end"]);
});
```

Append to `chat.ask.test.ts`:

```ts
it("initializes session identity and streaming state before any backend restore event", () => {
  createRoot(dispose => {
    const chat = createChatStore("seed-reload", true);
    expect(chat.sessionInfo.seed).toBe("seed-reload");
    expect(chat.isStreaming()).toBe(true);
    dispose();
  });
});
```

- [ ] **Step 2: Run the focused tests and verify both failures**

Run:

```powershell
pnpm exec vitest run src/runtime/viewLifecycle.test.ts src/runtime/sessionReplayBuffer.test.ts src/store/chat.ask.test.ts
```

Expected: both runtime modules are missing, `sessionInfo.seed` is currently empty, and `createChatStore` does not accept a restored streaming state.

- [ ] **Step 3: Implement frontend-only cleanup and authoritative initial seed**

Create `runtime/viewLifecycle.ts`:

```ts
export interface ViewRuntimeResource {
  dispose(): void;
}

export function cleanupViewResources(
  runtimes: Iterable<ViewRuntimeResource>,
  listeners: Iterable<() => void>,
  unlistenTheme?: () => void,
): void {
  for (const runtime of runtimes) runtime.dispose();
  for (const unlisten of listeners) {
    try { unlisten(); } catch { /* listener already removed */ }
  }
  unlistenTheme?.();
}
```

In `chat.ts`, add an optional restored streaming argument and initialize `sessionInfo` with the constructor seed:

```ts
export function createChatStore(seed: string, initialStreaming = false) {
  const [turns, setTurns] = createStore<Turn[]>([]);
  const [sessionInfo, setSessionInfo] = createStore<SessionInfo>({
    seed,
    model: "",
    context_tokens: 0,
    context_limit: 0,
    total_tokens: 0,
    prompt_cache_hit: 0,
    prompt_cache_miss: 0,
  });
  const [isStreaming, setIsStreaming] = createSignal(initialStreaming);
```

This replaces the existing function declaration plus the initial `turns`, `sessionInfo`, and `isStreaming` declarations; the remainder of `createChatStore` stays unchanged.

Create `runtime/sessionReplayBuffer.ts` with the tested `begin`, `handleLive`, `complete`, `abort`, and `clear` operations. `complete` must:

1. Count exact `JSON.stringify` signatures in the replay list.
2. Apply replay events in order.
3. Stop buffering the seed.
4. Drain live events in order, consuming one matching replay count before skipping an overlap.

`abort` performs `complete(seed, [], apply)`. `clear` drops all view-owned buffers. No backend or Tauri APIs belong in this module.

- [ ] **Step 4: Add raw runtimes and hydration to App**

Change imports in `App.tsx` so the chat import no longer imports transcript-only DTOs, and add:

```ts
import { createChatStore, type SessionMeta } from "./store/chat";
import { createRawSessionState, resolvePendingInteraction } from "./store/sessionEventReducer";
import {
  createSessionEventRuntime,
  loadReloadSnapshot,
  removeReloadSnapshot,
  type SessionEventRuntime,
} from "./store/sessionEventRuntime";
import { cleanupViewResources } from "./runtime/viewLifecycle";
import { createSessionReplayBuffer } from "./runtime/sessionReplayBuffer";
```

Add the raw runtime map and replay coordinator next to `rawSessions`:

```ts
const rawSessions = new Map<string, RawStore>();
const rawEventRuntimes = new Map<string, SessionEventRuntime>();
const sessionReplay = createSessionReplayBuffer();

function rawRuntimeForSeed(seed: string): SessionEventRuntime | undefined {
  return rawEventRuntimes.get(seed)
    ?? [...rawEventRuntimes.values()].find(runtime => runtime.current().seed === seed);
}
```

Replace the beginning of the async creation block inside `getOrCreateChatStore` with:

```ts
const initialRaw = loadReloadSnapshot(sessionStorage, seed)
  ?? createRawSessionState(seed);
const restoredStatus = initialRaw.turns.at(-1)?.status;
const s = createChatStore(
  seed,
  restoredStatus === "running" || restoredStatus === "waiting",
);
chatStores.set(seed, s);
const rawStore = createSignal(initialRaw);
rawSessions.set(seed, rawStore);
rawEventRuntimes.set(seed, createSessionEventRuntime({
  initialState: initialRaw,
  commit: next => rawStore[1](next),
  storage: sessionStorage,
}));
```

In the per-seed Tauri listener, replace the direct `handleAgentEvent` call with:

```ts
sessionReplay.handleLive(seed, e.payload, event => {
  handleAgentEvent(s, event, seed);
});
```

Replace `resumeSession`'s `existing && existing.sessionInfo.seed` early-return branch with one idempotent path. Mark replay before listener creation, attach the listener, resume the existing process, then replay and drain:

```ts
async function resumeSession(seed: string) {
  console.log("[App] resumeSession called, seed:", seed);
  sessionReplay.begin(seed);
  let chat: ChatStore | undefined;
  try {
    chat = await getOrCreateChatStore(seed);
    await invoke("cmd_resume_session", { seed });
    let replayed: Record<string, unknown>[] = [];
    try {
      replayed = await invoke<Record<string, unknown>[]>(
        "cmd_replay_session_events",
        { seed },
      );
    } catch (replayError) {
      console.warn("[App] lifecycle replay unavailable:", replayError);
    }
    sessionReplay.complete(seed, replayed, event => {
      handleAgentEvent(chat!, event, seed);
    });
    localStorage.setItem(LS_KEY, seed);
    setActiveSeed(seed);
    setHasChosenSession(true);
    setView("chat");
  } catch (error) {
    if (chat) {
      sessionReplay.abort(seed, event => handleAgentEvent(chat!, event, seed));
    } else {
      sessionReplay.abort(seed, () => {});
    }
    console.error("[App] resumeSession error:", error);
    setHasChosenSession(false);
    setView("home");
  }
}
```

`cmd_resume_session`/`ensure_agent` is already idempotent for a running registry entry. Removing the UI-side early return prevents a previously created-but-failed store from being mistaken for an attached process. Never use `cmd_load_more_turns` for this resynchronization: historical `TurnData` omits active/completed lifecycle state. The listener-first + replay + buffered-drain sequence closes the WebView swap gap without changing Ring loop behavior.

Replace the direct reducer call at the start of `handleAgentEvent` with:

```ts
const event = p as Agent2Ui;
rawEventRuntimes.get(listenerSeed)?.push(event);
```

Replace the direct raw signal update in `respondToPermission` with:

```ts
rawRuntimeForSeed(permission.seed)?.update(state =>
  resolvePendingInteraction(
    state,
    permission.request.tool_call_id,
    approved ? "approved" : "rejected",
  ),
);
```

- [ ] **Step 5: Remap/delete transient runtimes with their session**

The event runtime remains keyed by the original listener seed because the Tauri event channel name does not change after a fallback seed remap. In the `session_created` seed-remap branch, extend the existing raw-store remap with:

```ts
const runtime = rawEventRuntimes.get(listenerSeed);
if (runtime) {
  removeReloadSnapshot(sessionStorage, listenerSeed);
  runtime.flush();
}
```

In `deleteSession`, before deleting `rawSessions`, add:

```ts
const runtime = rawRuntimeForSeed(seed);
runtime?.dispose();
for (const [listenerSeed, candidate] of rawEventRuntimes) {
  if (candidate === runtime) rawEventRuntimes.delete(listenerSeed);
}
removeReloadSnapshot(sessionStorage, seed);
```

- [ ] **Step 6: Stop duplicate high-frequency transcript mutation**

Keep legacy calls for `turn_start`, `turn_end`, dashboard, ask, compact, permission, error, cancellation, and done. Change transcript-only cases to no-ops because the raw runtime is now authoritative:

```ts
case "round_delta":
case "tool_call_preview":
case "round_complete":
case "tool_results":
case "exec_progress":
case "tool_exec_delta":
  break;
```

In `session_restored`, remove `chat.loadTurnsFromRestore(turnsArr)`. In `more_turns`, remove `chat.prependTurns`; retain `chat.setHasMore(!!p.has_more)` if the existing UI still reads it.

Change `loadMoreTurns` to use raw turns:

```ts
const raw = activeRawSession();
const firstId = raw?.turns[0]?.turnId;
if (!firstId) return;
await invoke("cmd_load_more_turns", { seed, beforeTurnId: firstId });
```

Change slash-command undo to use raw turns:

```ts
const turns = activeRawSession()?.turns ?? [];
if (turns.length > 0) {
  void activeChat()?.undoTurn(turns[turns.length - 1]!.turnId);
}
```

Remove the `[APP_EVENT] tool_call_preview` debug log.

- [ ] **Step 7: Replace WebView cleanup agent shutdown with frontend-resource cleanup**

Replace the entire existing `onCleanup` body with:

```ts
onCleanup(() => {
  cleanupViewResources(
    rawEventRuntimes.values(),
    unlistenMap.values(),
    unlistenTheme,
  );
  rawEventRuntimes.clear();
  unlistenMap.clear();
  sessionReplay.clear();
});
```

There must be no `cmd_close_session` reference anywhere under `crates/deepx-tauri/src/`. Explicit session deletion and native window shutdown remain backend-owned paths outside this WebView cleanup.

- [ ] **Step 8: Run focused integration tests and static guards**

Run:

```powershell
cd F:\DeepX-Fork\crates\deepx-tauri
pnpm exec vitest run src/runtime/viewLifecycle.test.ts src/runtime/sessionReplayBuffer.test.ts src/store/chat.ask.test.ts src/store/sessionEventRuntime.test.ts src/store/sessionEventReducer.test.ts
rg -n "cmd_close_session|chat\.handleRoundDelta|chat\.handleToolCallPreview|chat\.handleRoundComplete|chat\.handleToolResults|chat\.handleExecProgress" src
```

Expected: all tests pass. `rg` exits 1 with no matches.

- [ ] **Step 9: Run all frontend tests and build**

Run:

```powershell
pnpm test:run
pnpm build
```

Expected: all Vitest files pass; `tsc --noEmit && vite build` exits 0.

- [ ] **Step 10: Verify backend scope remains frozen and commit only frontend files**

Run:

```powershell
cd F:\DeepX-Fork
git hash-object crates/deepx-tauri/src-tauri/Cargo.toml
git diff --name-only -- crates/deepx-msglp crates/deepx-tools crates/deepx-tauri/src-tauri
```

Expected hash: `5b80caae02abced664f6801fbd98fb512e3d979e`.

Expected uncommitted backend diff list: only the pre-existing `crates/deepx-tauri/src-tauri/Cargo.toml` status may appear. Task 5's four Tauri bridge source files are already committed; no Ring loop, protocol, or tool source file is changed.

Commit explicitly:

```powershell
git add crates/deepx-tauri/src/App.tsx crates/deepx-tauri/src/runtime/viewLifecycle.ts crates/deepx-tauri/src/runtime/viewLifecycle.test.ts crates/deepx-tauri/src/runtime/sessionReplayBuffer.ts crates/deepx-tauri/src/runtime/sessionReplayBuffer.test.ts crates/deepx-tauri/src/store/chat.ts crates/deepx-tauri/src/store/chat.ask.test.ts
git commit -m "fix(tauri): preserve sessions across webview reloads"
```

---

### Task 7: Execute the Phase 0 acceptance gate

**Files:**
- Verify only; do not create additional implementation files unless a failing acceptance test identifies a specific frontend defect.

**Interfaces:**
- Consumes: all changes from Tasks 1-6.
- Produces: evidence that automated tests, build, reload recovery, session switching, and backend freeze constraints hold together.

- [ ] **Step 1: Run the complete automated gate from fresh commands**

```powershell
cd F:\DeepX-Fork\crates\deepx-tauri
pnpm test:run
pnpm build
cd F:\DeepX-Fork
cargo test -p deepx-proto -- export_bindings
cargo check -p deepx-tauri --tests
git diff --check
```

Expected:

- Vitest: zero failed files and zero failed tests.
- TypeScript/Vite build: exit 0.
- Protocol export tests: exit 0.
- Tauri replay-cache assertions and command reachability compile check: exit 0.
- `git diff --check`: no whitespace errors.

- [ ] **Step 2: Confirm generated bindings remain synchronized after the final export**

```powershell
Compare-Object (Get-Content crates\deepx-proto\bindings\Agent2Ui.ts) (Get-Content crates\deepx-tauri\src\lib\types\Agent2Ui.ts)
```

Expected: no output.

- [ ] **Step 3: Run the Tauri manual reload smoke test**

Start the app:

```powershell
cd F:\DeepX-Fork\crates\deepx-tauri
pnpm tauri dev
```

Perform exactly these checks:

1. Open an existing session and confirm restored turns render.
2. Send a prompt that produces reasoning, at least one tool call, and a final answer.
3. While reasoning or tool output is streaming, right-click refresh the WebView.
4. Confirm the accumulated prompt/process/answer snapshot appears immediately after reload.
5. Confirm later deltas and the final answer continue without switching sessions or restarting DeepX.
6. Refresh again after completion and confirm the completed answer remains visible.
7. Switch to another session and back; confirm both transcripts remain correct.
8. Confirm DevTools contains no `Unhandled Agent2Ui event`, no stale Markdown overwrite, and no repeated `cmd_close_session` invocation.
9. Confirm backend logs show no agent kill caused by WebView refresh.

- [ ] **Step 4: Reconfirm the no-backend-refactor boundary**

```powershell
cd F:\DeepX-Fork
git status --short
git hash-object crates/deepx-tauri/src-tauri/Cargo.toml
git diff --name-only -- crates/deepx-msglp crates/deepx-tools crates/deepx-tauri/src-tauri
git diff --name-only 67de4448f4a4f631ca3b49d671fc28d052d82384 -- crates/deepx-msglp crates/deepx-tools crates/deepx-tauri/src-tauri
```

Expected:

- `Cargo.toml` hash remains `5b80caae02abced664f6801fbd98fb512e3d979e`.
- The uncommitted backend diff contains only the user-owned `Cargo.toml`.
- Relative to the plan baseline, Tauri Rust changes are exactly `registry.rs`, `commands/session.rs`, `compile_checks.rs`, and `src/lib.rs`; there are no `deepx-msglp` or `deepx-tools` changes.
- No workspace dependency, Ring loop, process architecture, or protocol transport change exists.

## Deferred Work

The following remain explicitly outside this plan even if they are desirable:

- Ring loop refactoring, recursion removal, global runtime-context removal, or multi-thread ownership.
- Standalone `deepx.exe`, WebSocket, named pipe, app-server, or Codex-compatible RPC implementation.
- Durable backend event journal or cross-process replay cursor.
- Full deletion of legacy `ChatStore` transcript fields.
- ratatui or WinUI 3 work.

After this frontend acceptance gate passes, the next permitted activity is a read-only backend gap audit and narrowly scoped fixes. Any structural Ring loop change requires a new design review and a separate approved plan.
