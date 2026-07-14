# Inline Interaction Wiring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Connect the existing ask-user, permission, and compaction presentation components to the active Tauri chat immediately above its composer.

**Architecture:** `App` keeps permission queue ownership and passes only the active session's permission plus its response callback into `ChatView`. `ChatView` combines that permission with its chat store's ask and compaction signals and renders one `InteractionDock` before `ComposerDock`.

**Tech Stack:** SolidJS, TypeScript, Vitest/jsdom, Tauri invoke API, CSS.

## Global Constraints

- Do not modify Rust or protocol DTOs.
- Do not use fullscreen or centered interaction overlays.
- High-risk approval remains solid red; rejection remains neutral.
- Permission takes precedence over ask-user; compact status may remain visible.
- Preserve existing ask, compact, permission queue, and composer pending-gate behavior.

---

### Task 1: Prove ChatView interaction placement and callbacks

**Files:**
- Create: `crates/deepx-tauri/src/components/ChatView.interactions.test.tsx`
- Modify: `crates/deepx-tauri/src/components/ChatView.tsx`

**Interfaces:**
- Consumes: `QueuedPermission`, chat store accessors `askState`, `isCompacting`, `compactText`, `compactResult`.
- Produces: optional `permission: () => QueuedPermission | null` and `onPermissionRespond(permission, approved, trustFolder)` props on `ChatView`.

- [ ] **Step 1: Write failing integration tests**

Render `ChatView` with a minimal fake chat store and assert:

```tsx
expect(host.querySelector(".interaction-dock")?.nextElementSibling)
  .toBe(host.querySelector(".composer-wrap"));
expect(host.querySelector(".ask-overlay")).toBeNull();
host.querySelector<HTMLButtonElement>(".interaction-option")!.click();
host.querySelector<HTMLButtonElement>(".interaction-submit")!.click();
expect(chat.submitAskAnswer).toHaveBeenCalledWith([
  { question_id: "q1", answer: "yes" },
]);
```

Add separate cases for permission callback/high-risk class and compact active/complete rendering.

- [ ] **Step 2: Run test to verify RED**

Run: `npm run test:run -- src/components/ChatView.interactions.test.tsx`

Expected: FAIL because `ChatView` still renders old ask overlays and has no dock/permission props.

- [ ] **Step 3: Implement the minimal ChatView dock**

Replace `AskDialog`/`AskForm` imports and render calls with:

```tsx
<Show when={hasInteractions()}>
  <InteractionDock>
    <Show when={showCompactStatus()}>
      <CompactStatusRow
        active={chat().isCompacting()}
        status={chat().isCompacting() ? "active" : "complete"}
        text={chat().compactText()}
        turnsCompacted={chat().compactResult() ?? undefined}
      />
    </Show>
    <Show when={permission()} fallback={
      <Show when={chat().askState().show}>
        <AskUserPrompt
          questions={chat().askState().questions}
          onSubmit={chat().submitAskAnswer}
          onDismiss={chat().dismissAsk}
        />
      </Show>
    }>
      {(item) => (
        <PermissionPrompt
          request={item().request}
          onRespond={(approved, trust) => props.onPermissionRespond?.(item(), approved, trust)}
        />
      )}
    </Show>
  </InteractionDock>
</Show>
```

Place it immediately before `ComposerDock`.

- [ ] **Step 4: Run test to verify GREEN**

Run: `npm run test:run -- src/components/ChatView.interactions.test.tsx`

Expected: all new tests PASS.

### Task 2: Move permission ownership through App

**Files:**
- Modify: `crates/deepx-tauri/src/App.tsx`
- Test: `crates/deepx-tauri/src/components/ChatView.interactions.test.tsx`

**Interfaces:**
- Consumes: `permissionQueue.active()`, `activeSeed()`, `cmd_permission_response`.
- Produces: `ChatView.permission` and `ChatView.onPermissionRespond` bindings.

- [ ] **Step 1: Make the production wiring compile against the tested props**

Add an active-session selector:

```tsx
const activeChatPermission = () => {
  const item = permissionQueue.active();
  return item?.seed === activeSeed() ? item : null;
};
```

Move the existing response body into `respondToPermission(item, approved, trustFolder)` and pass both into `ChatView`. Remove the trailing App-level `interaction-overlay` permission render.

- [ ] **Step 2: Verify targeted behavior**

Run: `npm run test:run -- src/components/ChatView.interactions.test.tsx src/components/interactions`

Expected: all targeted tests PASS.

### Task 3: Verify and commit the complete slice

**Files:**
- Modify: only `App.tsx`, `ChatView.tsx`, the new ChatView test, and any narrowly required interaction CSS/test file.

- [ ] **Step 1: Run complete validation**

```powershell
npm run test:run
npm run build
git diff --check
git diff --stat
git status --short
```

Expected: frontend tests and build exit 0; diff contains no Rust/protocol changes or unrelated files.

- [ ] **Step 2: Commit**

```powershell
git add crates/deepx-tauri/src/App.tsx crates/deepx-tauri/src/components/ChatView.tsx crates/deepx-tauri/src/components/ChatView.interactions.test.tsx
git commit -m "refactor(tauri): wire inline interaction dock"
```
