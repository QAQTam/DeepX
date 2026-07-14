# DeepX Tauri App Shell and Composer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the permanent engineering dashboard shell with task navigation, a focused thread workspace, an overlay environment panel, and a floating composer that queues follow-up work.

**Architecture:** App shell state is separated from session protocol state. Existing commands supply session, Git, workspace, context, and model data; the environment panel overlays the transcript and the composer owns a per-session in-memory follow-up queue.

**Tech Stack:** SolidJS 1.9, TypeScript 6, Tauri invoke/listen APIs, Vitest 4, CSS.

## Global Constraints

- This plan starts only after the conversation plan passes.
- Sidebar is the sole task/session switcher; do not retain visible open tabs.
- Environment UI never reserves transcript width.
- Composer and transcript use the same width token.
- Queued work never auto-runs after application restart.
- Do not delete legacy shell components until the integration plan.

---

### Task 1: Build task navigation and the focused thread header

**Files:**
- Create: `crates/deepx-tauri/src/components/shell/TaskSidebar.tsx`
- Create: `crates/deepx-tauri/src/components/shell/ThreadHeader.tsx`
- Create: `crates/deepx-tauri/src/components/shell/TaskSidebar.test.tsx`
- Create: `crates/deepx-tauri/src/styles/shell.css`

**Interfaces:**
- Consumes: `SessionMeta[]`, a `sessionTitleBySeed` map populated from dashboard events, active seed, and resume/new/delete/compact/undo callbacks.
- Produces: one session switcher and task-title fallback logic.

- [ ] **Step 1: Write navigation and title tests**

```tsx
it("uses the approved title fallback order", () => {
  const named = { last_summary: "Summary", seed: "abcdef12" } as SessionMeta;
  const summarized = { last_summary: "Summary", seed: "abcdef12" } as SessionMeta;
  const seeded = { last_summary: "", seed: "abcdef12" } as SessionMeta;
  expect(taskTitle(named, "Named")).toBe("Named");
  expect(taskTitle(summarized)).toBe("Summary");
  expect(taskTitle(seeded)).toBe("abcdef12");
});

it("renders sessions once and no open-tab navigation", () => {
  const root = renderSidebar(twoSessions());
  expect(root.querySelectorAll("[data-task-session]")).toHaveLength(2);
  expect(root.querySelector(".open-tabs")).toBeNull();
});
```

- [ ] **Step 2: Run the test to verify it fails**

```powershell
pnpm --dir crates/deepx-tauri exec vitest run src/components/shell/TaskSidebar.test.tsx
```

Expected: FAIL because shell components do not exist.

- [ ] **Step 3: Implement sidebar and header**

```ts
export function taskTitle(session: SessionMeta, dashboardTitle?: string): string {
  return dashboardTitle?.trim()
    || session.last_summary?.trim()
    || session.seed.slice(0, 8);
}
```

Render only New Task, Skills, Settings, and actual sessions. Put delete in a row overflow menu. `ThreadHeader` exposes Open Location, Environment, and a compact menu for undo/compact/delete/close.

- [ ] **Step 4: Verify shell component tests**

```powershell
pnpm --dir crates/deepx-tauri exec vitest run src/components/shell/TaskSidebar.test.tsx
```

Expected: PASS with no unsupported navigation labels.

- [ ] **Step 5: Commit task navigation**

```powershell
git add crates/deepx-tauri/src/components/shell/TaskSidebar.tsx crates/deepx-tauri/src/components/shell/ThreadHeader.tsx crates/deepx-tauri/src/components/shell/TaskSidebar.test.tsx crates/deepx-tauri/src/styles/shell.css
git commit -m "feat(tauri): add task-focused shell navigation"
```

### Task 2: Build the non-reserving environment popover

**Files:**
- Create: `crates/deepx-tauri/src/store/environmentStore.ts`
- Create: `crates/deepx-tauri/src/store/environmentStore.test.ts`
- Create: `crates/deepx-tauri/src/components/shell/EnvironmentPopover.tsx`
- Create: `crates/deepx-tauri/src/components/shell/EnvironmentPopover.test.tsx`
- Modify: `crates/deepx-tauri/src/styles/shell.css`

**Interfaces:**
- Consumes: `cmd_get_git_diff`, `cmd_get_git_branch`, `cmd_get_workspace`, session info, and reduced `code_delta` totals.
- Produces: `EnvironmentState`, refresh actions, and overlay UI.

- [ ] **Step 1: Write initial-plus-incremental delta test**

```ts
it("combines initial git totals with later code deltas", () => {
  const initial = environmentFromGit([{ additions: 10, deletions: 3 }]);
  const next = applyCodeDelta(initial, { lines_added: 4, lines_removed: 2, files_created: 0, files_deleted: 0 });
  expect(next.changes).toEqual({ additions: 14, deletions: 5 });
});
```

- [ ] **Step 2: Run the test to verify it fails**

```powershell
pnpm --dir crates/deepx-tauri exec vitest run src/store/environmentStore.test.ts src/components/shell/EnvironmentPopover.test.tsx
```

Expected: FAIL because environment modules do not exist.

- [ ] **Step 3: Implement independent row loading and overlay layout**

```ts
export type EnvironmentRow<T> =
  | { status: "loading" }
  | { status: "ready"; value: T }
  | { status: "unavailable"; message: string };
```

Load Git, branch, and workspace independently so one failure does not close the panel. Position the panel absolutely/fixed relative to `ThreadWorkspace`; do not modify transcript padding or grid columns when open.

- [ ] **Step 4: Verify environment tests**

```powershell
pnpm --dir crates/deepx-tauri exec vitest run src/store/environmentStore.test.ts src/components/shell/EnvironmentPopover.test.tsx
```

Expected: PASS; one unavailable row leaves other rows visible.

- [ ] **Step 5: Commit the environment panel**

```powershell
git add crates/deepx-tauri/src/store/environmentStore.ts crates/deepx-tauri/src/store/environmentStore.test.ts crates/deepx-tauri/src/components/shell/EnvironmentPopover.tsx crates/deepx-tauri/src/components/shell/EnvironmentPopover.test.tsx crates/deepx-tauri/src/styles/shell.css
git commit -m "feat(tauri): add overlay environment panel"
```

### Task 3: Implement the per-session follow-up queue

**Files:**
- Create: `crates/deepx-tauri/src/store/followUpQueue.ts`
- Create: `crates/deepx-tauri/src/store/followUpQueue.test.ts`

**Interfaces:**
- Consumes: current turn status, unresolved-gate status, and `cmd_send_message` callback.
- Produces: `createFollowUpQueue(seed)`, queue editing, and `drainAfterTurnEnd()`.

- [ ] **Step 1: Write queue safety tests**

```ts
it("waits for turn_end and no unresolved gate before sending", async () => {
  const sent: string[] = [];
  const q = createFollowUpQueue("seed-a", text => { sent.push(text); return Promise.resolve(); });
  q.enqueue("next change");
  await q.drainAfterTurnEnd({ hasPendingGate: true });
  expect(sent).toEqual([]);
  await q.drainAfterTurnEnd({ hasPendingGate: false });
  expect(sent).toEqual(["next change"]);
});

it("does not persist executable queue entries", () => {
  const q = createFollowUpQueue("seed-a", async () => {});
  q.enqueue("dangerous follow-up");
  expect(localStorage.getItem("deepx:follow-ups:seed-a")).toBeNull();
});
```

- [ ] **Step 2: Run the tests to verify they fail**

```powershell
pnpm --dir crates/deepx-tauri exec vitest run src/store/followUpQueue.test.ts
```

Expected: FAIL because the queue does not exist.

- [ ] **Step 3: Implement explicit queue transitions**

```ts
export type FollowUpItem = { id: string; text: string; files: string[] };

export function createFollowUpQueue(seed: string, send: (text: string, files: string[]) => Promise<void>) {
  const [items, setItems] = createSignal<FollowUpItem[]>([]);
  let draining = false;
  async function drainAfterTurnEnd({ hasPendingGate }: { hasPendingGate: boolean }) {
    if (draining || hasPendingGate || items().length === 0) return;
    draining = true;
    const item = items()[0];
    try { await send(item.text, item.files); setItems(list => list.slice(1)); }
    finally { draining = false; }
  }
  return { items, enqueue, update, remove, clear, drainAfterTurnEnd };
}
```

Implement `enqueue/update/remove/clear` as in-memory operations. Cancellation exposes a confirmation state; it does not call `drainAfterTurnEnd` automatically.

- [ ] **Step 4: Verify queue tests**

```powershell
pnpm --dir crates/deepx-tauri exec vitest run src/store/followUpQueue.test.ts
```

Expected: PASS for gating, ordering, editing, removal, and non-persistence.

- [ ] **Step 5: Commit queue behavior**

```powershell
git add crates/deepx-tauri/src/store/followUpQueue.ts crates/deepx-tauri/src/store/followUpQueue.test.ts
git commit -m "feat(tauri): queue per-session follow-up input"
```

### Task 4: Build the floating composer

**Files:**
- Create: `crates/deepx-tauri/src/components/composer/ComposerDock.tsx`
- Create: `crates/deepx-tauri/src/components/composer/ComposerQueue.tsx`
- Create: `crates/deepx-tauri/src/components/composer/ComposerDock.test.tsx`
- Create: `crates/deepx-tauri/src/styles/composer.css`

**Interfaces:**
- Consumes: follow-up queue, send/stop, attachments, mode, Skills, permission mode, and restored error text.
- Produces: editable running-state composer and queue controls.

- [ ] **Step 1: Write running-state composer test**

```tsx
it("queues input instead of sending directly while a turn runs", () => {
  const h = renderComposer({ isStreaming: true });
  h.type("apply this after the current turn");
  h.submit();
  expect(h.send).not.toHaveBeenCalled();
  expect(h.queue.items()).toHaveLength(1);
  expect(h.root.textContent).toContain("1 follow-up queued");
});
```

- [ ] **Step 2: Run the test to verify it fails**

```powershell
pnpm --dir crates/deepx-tauri exec vitest run src/components/composer/ComposerDock.test.tsx
```

Expected: FAIL because composer components do not exist.

- [ ] **Step 3: Implement composer behavior**

Route submit based on status:

```ts
if (props.isStreaming()) {
  props.followUps.enqueue(text, files());
} else if (!props.hasPendingGate()) {
  await props.onSend(text, files());
}
```

Keep file chips, slash menu, Skill activation, Plan/Code mode, permission indicator, model indication, send/stop, and restored text. The running send control becomes Stop while the textarea remains editable.

- [ ] **Step 4: Verify composer tests**

```powershell
pnpm --dir crates/deepx-tauri exec vitest run src/components/composer src/store/followUpQueue.test.ts
```

Expected: PASS for idle send, running queue, stop, attachments, and pending-gate blocking.

- [ ] **Step 5: Commit the composer**

```powershell
git add crates/deepx-tauri/src/components/composer crates/deepx-tauri/src/styles/composer.css
git commit -m "feat(tauri): add floating queued composer"
```

### Task 5: Assemble the new App shell behind the comparison flag

**Files:**
- Create: `crates/deepx-tauri/src/components/shell/AppShell.tsx`
- Create: `crates/deepx-tauri/src/components/shell/ThreadWorkspace.tsx`
- Create: `crates/deepx-tauri/src/components/shell/EmptyWorkspace.tsx`
- Create: `crates/deepx-tauri/src/components/shell/AppShell.test.tsx`
- Modify: `crates/deepx-tauri/src/App.tsx:486`
- Modify: `crates/deepx-tauri/src/main.tsx`

**Interfaces:**
- Consumes: all shell, conversation, interaction, and composer components.
- Produces: the approved shell without permanent status/info panels or visible open tabs on the new path.

- [ ] **Step 1: Write shell hierarchy test**

```tsx
it("renders the focused shell without permanent engineering panels", () => {
  const root = renderAppShell(appFixture());
  expect(root.querySelector("[data-task-sidebar]")).not.toBeNull();
  expect(root.querySelector("[data-thread-workspace]")).not.toBeNull();
  expect(root.querySelector("[data-composer-dock]")).not.toBeNull();
  expect(root.querySelector(".status-panel")).toBeNull();
  expect(root.querySelector(".info-bar")).toBeNull();
  expect(root.querySelector(".open-tabs")).toBeNull();
});
```

- [ ] **Step 2: Run the test to verify it fails**

```powershell
pnpm --dir crates/deepx-tauri exec vitest run src/components/shell/AppShell.test.tsx
```

Expected: FAIL because AppShell does not exist.

- [ ] **Step 3: Assemble the new path**

```tsx
<AppShell
  sidebar={<TaskSidebar {...taskProps} />}
  header={<ThreadHeader {...headerProps} />}
  transcript={<ConversationTranscript turns={conversationViews()} />}
  composer={<ComposerDock {...composerProps} />}
  environment={<EnvironmentPopover {...environmentProps} />}
/>
```

When no session is active, render `EmptyWorkspace` with a new-task composer and recent tasks. Keep old components importable only for the comparison fallback.

- [ ] **Step 4: Run the shell gate**

```powershell
pnpm --dir crates/deepx-tauri run test:run
pnpm --dir crates/deepx-tauri run build
```

Expected: PASS; new path has no permanent right rail or telemetry strip.

- [ ] **Step 5: Commit shell assembly**

```powershell
git add crates/deepx-tauri/src/components/shell crates/deepx-tauri/src/App.tsx crates/deepx-tauri/src/main.tsx
git commit -m "feat(tauri): assemble focused desktop shell"
```
