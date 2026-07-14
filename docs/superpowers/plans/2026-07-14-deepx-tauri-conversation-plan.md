# DeepX Tauri Conversation Transcript Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace round-by-round messages and ToolCards with user prompts, one process disclosure, interaction gates, and a final assistant answer.

**Architecture:** Components consume `TurnViewModel` only. A turn-owned disclosure controls one timeline whose children use stable IDs; pending permission, ask-user, and plan interactions are promoted temporarily and recorded into the process after resolution.

**Tech Stack:** SolidJS 1.9, TypeScript 6, Vitest 4, existing Markdown/Shiki/ANSI/diff utilities, CSS.

## Global Constraints

- This plan starts only after the foundation plan passes.
- Do not read raw `RoundData` or `ToolResultDef` inside visual components.
- Use `<Index>` or stable keyed access for streaming items; do not remount rows on chunk updates.
- Completed turns default collapsed; failed/cancelled turns default expanded.
- Only one process item detail is expanded per turn.
- Keep the old renderer behind a development-only comparison flag until Task 5 passes.

---

### Task 1: Build the process disclosure state machine

**Files:**
- Create: `crates/deepx-tauri/src/components/process/ProcessDisclosure.tsx`
- Create: `crates/deepx-tauri/src/components/process/ProcessDisclosure.test.tsx`
- Create: `crates/deepx-tauri/src/styles/process.css`
- Create: `crates/deepx-tauri/src/styles/tokens.css`

**Interfaces:**
- Consumes: `TurnViewModel["process"]`.
- Produces: `ProcessDisclosure` with `defaultOpenForStatus(status)` and an `onOpenChange` callback.

- [ ] **Step 1: Write lifecycle tests**

```tsx
it("opens running and failed traces but collapses completed traces", () => {
  expect(defaultOpenForStatus("running")).toBe(true);
  expect(defaultOpenForStatus("waiting")).toBe(true);
  expect(defaultOpenForStatus("failed")).toBe(true);
  expect(defaultOpenForStatus("cancelled")).toBe(true);
  expect(defaultOpenForStatus("completed")).toBe(false);
});

it("forces a running trace closed when it completes", async () => {
  const { setStatus, disclosure } = renderProcessDisclosure("running");
  expect(disclosure().getAttribute("aria-expanded")).toBe("true");
  setStatus("completed");
  await Promise.resolve();
  expect(disclosure().getAttribute("aria-expanded")).toBe("false");
});
```

- [ ] **Step 2: Run the test to verify it fails**

```powershell
pnpm --dir crates/deepx-tauri exec vitest run src/components/process/ProcessDisclosure.test.tsx
```

Expected: FAIL because the component does not exist.

- [ ] **Step 3: Implement disclosure semantics and tokens**

```ts
export function defaultOpenForStatus(status: TurnViewModel["process"]["status"]): boolean {
  return status !== "completed";
}
```

The button uses `aria-expanded` and `aria-controls`. Labels are status-driven:

```ts
const label = () => ({
  running: `Processing ${formatElapsed(props.elapsedMs)}`,
  waiting: "Needs your approval",
  completed: `Processed in ${formatElapsed(props.elapsedMs)}`,
  failed: "Processing failed",
  cancelled: "Stopped",
}[props.status]);
```

Add shared CSS variables for transcript width (`760px`), composer width (`760px`), subtle borders, neutral surfaces, danger red, and reduced motion.

- [ ] **Step 4: Verify disclosure tests and accessibility attributes**

```powershell
pnpm --dir crates/deepx-tauri exec vitest run src/components/process/ProcessDisclosure.test.tsx
```

Expected: PASS; rendered button has a visible label and correct ARIA state.

- [ ] **Step 5: Commit the disclosure**

```powershell
git add crates/deepx-tauri/src/components/process/ProcessDisclosure.tsx crates/deepx-tauri/src/components/process/ProcessDisclosure.test.tsx crates/deepx-tauri/src/styles/process.css crates/deepx-tauri/src/styles/tokens.css
git commit -m "feat(tauri): add turn process disclosure"
```

### Task 2: Render compact process events and lazy details

**Files:**
- Create: `crates/deepx-tauri/src/components/process/ProcessTimeline.tsx`
- Create: `crates/deepx-tauri/src/components/process/ProcessEventRow.tsx`
- Create: `crates/deepx-tauri/src/components/process/ProcessDetail.tsx`
- Create: `crates/deepx-tauri/src/components/process/ProcessTimeline.test.tsx`
- Modify: `crates/deepx-tauri/src/styles/process.css`

**Interfaces:**
- Consumes: `ProcessItem[]` and existing `renderDiffHtml`/ANSI conversion helpers.
- Produces: one-line event rows, one expanded detail at a time, and bounded output previews.

- [ ] **Step 1: Write aggregation-rendering and stable-detail tests**

```tsx
it("renders an aggregate as one row and keeps failures separate", () => {
  const root = renderTimeline([
    groupItem("reads", "Viewed 4 files", fourReadChildren()),
    toolItem("build", "Frontend build failed", false),
  ]);
  expect(root.textContent).toContain("Viewed 4 files");
  expect(root.textContent).toContain("Frontend build failed");
  expect(root.querySelectorAll("[data-process-row]")).toHaveLength(2);
});

it("preserves an open tool detail when its streaming output changes", async () => {
  const h = renderStreamingTimeline();
  h.open("exec-1");
  h.append("exec-1", "next chunk");
  await Promise.resolve();
  expect(h.row("exec-1").getAttribute("aria-expanded")).toBe("true");
});
```

- [ ] **Step 2: Run the tests to verify they fail**

```powershell
pnpm --dir crates/deepx-tauri exec vitest run src/components/process/ProcessTimeline.test.tsx
```

Expected: FAIL because timeline components do not exist.

- [ ] **Step 3: Implement stable rows and bounded details**

Use an item ID signal rather than local state inside a remountable row:

```tsx
const [expandedId, setExpandedId] = createSignal<string | null>(null);

<Index each={props.items}>
  {(item) => (
    <ProcessEventRow
      item={item()}
      expanded={expandedId() === item().id}
      onToggle={() => setExpandedId(id => id === item().id ? null : item().id)}
    />
  )}
</Index>
```

`ProcessDetail` renders at most the configured preview lines until `Show full output` is clicked. Failures receive semantic text and a restrained danger indicator, not a full red card.

- [ ] **Step 4: Verify component tests and existing diff tests**

```powershell
pnpm --dir crates/deepx-tauri exec vitest run src/components/process/ProcessTimeline.test.tsx src/lib
```

Expected: PASS; streaming update does not close the row.

- [ ] **Step 5: Commit process rendering**

```powershell
git add crates/deepx-tauri/src/components/process crates/deepx-tauri/src/styles/process.css
git commit -m "feat(tauri): render compact process timelines"
```

### Task 3: Build the completed turn transcript

**Files:**
- Create: `crates/deepx-tauri/src/components/conversation/UserPromptBubble.tsx`
- Create: `crates/deepx-tauri/src/components/conversation/AssistantAnswer.tsx`
- Create: `crates/deepx-tauri/src/components/conversation/TurnGroup.tsx`
- Create: `crates/deepx-tauri/src/components/conversation/ConversationTranscript.tsx`
- Create: `crates/deepx-tauri/src/components/conversation/TurnGroup.test.tsx`
- Create: `crates/deepx-tauri/src/styles/conversation.css`

**Interfaces:**
- Consumes: `TurnViewModel[]`, existing `MarkdownBody`, and the process components.
- Produces: the approved three-node completed-turn hierarchy.

- [ ] **Step 1: Write the top-level hierarchy test**

```tsx
it("renders only prompt, process disclosure, and final answer for a completed turn", () => {
  const root = renderTurn(completedTurnView());
  const turn = root.querySelector("[data-turn]")!;
  expect(Array.from(turn.children).map(node => node.getAttribute("data-part"))).toEqual([
    "user-prompt", "process", "assistant-answer",
  ]);
  expect(root.querySelector(".tool-card")).toBeNull();
  expect(root.querySelector(".msg-avatar")).toBeNull();
});
```

- [ ] **Step 2: Run the test to verify it fails**

```powershell
pnpm --dir crates/deepx-tauri exec vitest run src/components/conversation/TurnGroup.test.tsx
```

Expected: FAIL because conversation components do not exist.

- [ ] **Step 3: Implement flat transcript components**

```tsx
export default function TurnGroup(props: { turn: TurnViewModel }) {
  return <article data-turn={props.turn.turnId}>
    <UserPromptBubble text={props.turn.userPrompt} />
    <ProcessDisclosure process={props.turn.process} />
    <Show when={props.turn.finalAnswer}>
      {(answer) => <AssistantAnswer markdown={answer().markdown} />}
    </Show>
  </article>;
}
```

`AssistantAnswer` wraps `MarkdownBody` without avatar/role chrome. `ConversationTranscript` owns scroll anchoring and jump-to-bottom behavior.

- [ ] **Step 4: Verify transcript and Markdown tests**

```powershell
pnpm --dir crates/deepx-tauri exec vitest run src/components/conversation src/components/MarkdownBody.tsx
```

Expected: PASS; completed hierarchy has exactly the approved parts.

- [ ] **Step 5: Commit the transcript**

```powershell
git add crates/deepx-tauri/src/components/conversation crates/deepx-tauri/src/styles/conversation.css
git commit -m "feat(tauri): add focused conversation transcript"
```

### Task 4: Replace permission and ask-user visual gates

**Files:**
- Modify: `crates/deepx-tauri/src/store/permissionQueue.ts`
- Create: `crates/deepx-tauri/src/components/interactions/PermissionPrompt.tsx`
- Create: `crates/deepx-tauri/src/components/interactions/AskUserPrompt.tsx`
- Create: `crates/deepx-tauri/src/components/interactions/PlanApprovalPrompt.tsx`
- Create: `crates/deepx-tauri/src/components/interactions/PermissionPrompt.test.tsx`
- Create: `crates/deepx-tauri/src/styles/interactions.css`

**Interfaces:**
- Consumes: generated `PermissionRisk`, current permission queue, ask state, and plan actions.
- Produces: inline pending interactions and resolved process-history records.

- [ ] **Step 1: Write approved risk-style tests**

```tsx
it.each([
  ["low", "approval-low"],
  ["medium", "approval-medium"],
  ["high", "approval-high"],
])("maps %s risk to %s styling", (risk, className) => {
  const root = renderPermission({ risk });
  expect(root.querySelector("[data-approve]")?.classList.contains(className)).toBe(true);
});

it("keeps rejection neutral when approval is high risk", () => {
  const root = renderPermission({ risk: "high" });
  expect(root.querySelector("[data-reject]")?.classList.contains("approval-high")).toBe(false);
});
```

- [ ] **Step 2: Run the test to verify it fails**

```powershell
pnpm --dir crates/deepx-tauri exec vitest run src/components/interactions/PermissionPrompt.test.tsx
```

Expected: FAIL because new interaction components do not exist.

- [ ] **Step 3: Implement inline interactions**

Use backend risk directly:

```ts
export const approvalClass = (risk: PermissionRisk) => ({
  low: "approval-low",
  medium: "approval-medium",
  high: "approval-high",
}[risk]);
```

Permission prompts show reason, normalized paths, consequence, and grant scope. Ask-user and Plan Review retain existing submit/dismiss/action commands. On resolution, the raw reducer records an `InteractionRecord` and removes the pending interaction.

- [ ] **Step 4: Verify all interaction lifecycle tests**

```powershell
pnpm --dir crates/deepx-tauri exec vitest run src/components/interactions src/store/permissionQueue.test.ts src/store/chat.ask.test.ts
```

Expected: PASS for approval, denial, retry, single ask, and batch ask.

- [ ] **Step 5: Commit interaction gates**

```powershell
git add crates/deepx-tauri/src/store/permissionQueue.ts crates/deepx-tauri/src/components/interactions crates/deepx-tauri/src/styles/interactions.css
git commit -m "feat(tauri): integrate risk-aware conversation gates"
```

### Task 5: Integrate the new transcript behind a temporary comparison flag

**Files:**
- Modify: `crates/deepx-tauri/src/components/ChatView.tsx`
- Modify: `crates/deepx-tauri/src/main.tsx`
- Create: `crates/deepx-tauri/src/presentation/useConversationView.ts`
- Create: `crates/deepx-tauri/src/presentation/useConversationView.test.ts`

**Interfaces:**
- Consumes: raw session state and `projectTurn`.
- Produces: reactive `TurnViewModel[]` and development-only renderer selection.

- [ ] **Step 1: Write live/restore parity test**

```ts
it("projects restored history and equivalent live events identically", () => {
  const restored = viewsFromRestore(turnFixture());
  const live = viewsFromEvents(eventFixture());
  expect(live).toEqual(restored);
});
```

- [ ] **Step 2: Run the parity test to verify it fails**

```powershell
pnpm --dir crates/deepx-tauri exec vitest run src/presentation/useConversationView.test.ts
```

Expected: FAIL because the reactive adapter does not exist.

- [ ] **Step 3: Integrate the new renderer**

```tsx
const useNewConversation = () =>
  !import.meta.env.DEV || localStorage.getItem("deepx:new-conversation-ui") !== "0";

<Show when={useNewConversation()} fallback={<LegacyMessageList {...legacyProps} />}>
  <ConversationTranscript turns={conversationViews()} />
</Show>
```

Import `tokens.css`, `conversation.css`, `process.css`, and `interactions.css` in `main.tsx`. Keep the legacy import only until the integration plan removes it.

- [ ] **Step 4: Run the conversation gate**

```powershell
pnpm --dir crates/deepx-tauri run test:run
pnpm --dir crates/deepx-tauri run build
```

Expected: PASS with both development comparison paths and one production path.

- [ ] **Step 5: Commit conversation integration**

```powershell
git add crates/deepx-tauri/src/components/ChatView.tsx crates/deepx-tauri/src/main.tsx crates/deepx-tauri/src/presentation/useConversationView.ts crates/deepx-tauri/src/presentation/useConversationView.test.ts
git commit -m "feat(tauri): integrate the new conversation renderer"
```
