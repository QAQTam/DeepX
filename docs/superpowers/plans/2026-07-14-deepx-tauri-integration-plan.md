# DeepX Tauri Integration, Native Window, and Legacy Cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish Skills/Settings, add native-window-quality chrome, validate the full lifecycle, and permanently remove the legacy UI and comparison flag.

**Architecture:** Secondary views reuse the new shell tokens and existing commands. A custom Tauri titlebar uses official window APIs. Final lifecycle fixtures validate raw events through projection and DOM before the old components and CSS are deleted.

**Tech Stack:** Tauri 2 window API, SolidJS 1.9, TypeScript 6, Vitest 4, Rust 2024, CSS.

## Global Constraints

- This plan starts only after the shell plan passes.
- No additional UI framework or icon package is introduced.
- Custom window controls must preserve drag, minimize, maximize/restore, close, DPI, and keyboard focus behavior.
- Settings and Skills use only currently wired commands.
- Legacy deletion happens only after lifecycle and visual gates pass on the new path.
- The final production tree contains no comparison flag or legacy renderer.

---

### Task 1: Move Skills and usage into focused secondary views

**Files:**
- Create: `crates/deepx-tauri/src/components/views/SkillsView.tsx`
- Create: `crates/deepx-tauri/src/components/views/SkillsView.test.tsx`
- Create: `crates/deepx-tauri/src/components/views/UsageView.tsx`
- Create: `crates/deepx-tauri/src/styles/views.css`
- Modify: `crates/deepx-tauri/src/App.tsx`

**Interfaces:**
- Consumes: skill catalog/active names, activate/unload/reload commands, and existing token statistics.
- Produces: dedicated Skills and Usage destinations without a permanent status panel or Home dashboard.

- [ ] **Step 1: Write wired-action tests**

```tsx
it("renders project and user skills and invokes the existing activation command", () => {
  const h = renderSkillsView(skillFixture());
  expect(h.root.textContent).toContain("Project skills");
  expect(h.root.textContent).toContain("User skills");
  h.activate("frontend-design");
  expect(h.invoke).toHaveBeenCalledWith("cmd_activate_skill", {
    seed: "seed-a", name: "frontend-design",
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

```powershell
pnpm --dir crates/deepx-tauri exec vitest run src/components/views/SkillsView.test.tsx
```

Expected: FAIL because `SkillsView` does not exist.

- [ ] **Step 3: Implement secondary views**

Group skills by scope:

```ts
const projectSkills = () => props.catalog().filter(skill => skill.scope === "project");
const userSkills = () => props.catalog().filter(skill => skill.scope === "user");
```

Expose source path, active state, activation, unloading, and refresh. Move `TokenChart` into `UsageView`; remove it from the no-session workspace.

- [ ] **Step 4: Verify secondary views**

```powershell
pnpm --dir crates/deepx-tauri exec vitest run src/components/views
pnpm --dir crates/deepx-tauri run build
```

Expected: PASS; no unsupported destination is rendered.

- [ ] **Step 5: Commit secondary views**

```powershell
git add crates/deepx-tauri/src/components/views crates/deepx-tauri/src/styles/views.css crates/deepx-tauri/src/App.tsx
git commit -m "feat(tauri): add focused skills and usage views"
```

### Task 2: Restructure Settings with categorized navigation

**Files:**
- Modify: `crates/deepx-tauri/src/components/SettingsView.tsx`
- Create: `crates/deepx-tauri/src/components/SettingsView.test.tsx`
- Replace: `crates/deepx-tauri/src/styles/settings.css`

**Interfaces:**
- Consumes: existing config load/save, migration, provider, model, subagent, interface, compliance, and database behavior.
- Produces: categorized settings navigation with one visible section at a time.

- [ ] **Step 1: Write behavior-preservation tests**

```tsx
it("switches categories without losing unsaved form state", async () => {
  const h = renderSettings(configFixture());
  h.setField("model", "deepseek-reasoner");
  h.openCategory("Interface");
  h.openCategory("Model");
  expect(h.field("model").value).toBe("deepseek-reasoner");
});

it("saves through the existing command payload", () => {
  const h = renderSettings(configFixture());
  h.save();
  expect(h.invoke).toHaveBeenCalledWith("cmd_save_config", expect.objectContaining({
    model: "deepseek-chat", databaseEnabled: true,
  }));
});
```

- [ ] **Step 2: Run the tests to verify category behavior is absent**

```powershell
pnpm --dir crates/deepx-tauri exec vitest run src/components/SettingsView.test.tsx
```

Expected: FAIL because categorized navigation helpers are absent.

- [ ] **Step 3: Refactor layout without changing command behavior**

Use a category signal:

```ts
type SettingsCategory = "provider" | "model" | "subagent" | "interface" | "compliance" | "database" | "usage";
const [category, setCategory] = createSignal<SettingsCategory>("provider");
```

Retain one owner for each form signal and the existing save/migration functions. Render category navigation on the left and the selected form section on the right. Do not split save state across child-local copies.

- [ ] **Step 4: Verify Settings and migration behavior**

```powershell
pnpm --dir crates/deepx-tauri exec vitest run src/components/SettingsView.test.tsx
pnpm --dir crates/deepx-tauri run build
```

Expected: PASS for category switching, save payload, and migration state.

- [ ] **Step 5: Commit Settings redesign**

```powershell
git add crates/deepx-tauri/src/components/SettingsView.tsx crates/deepx-tauri/src/components/SettingsView.test.tsx crates/deepx-tauri/src/styles/settings.css
git commit -m "refactor(tauri): organize settings into focused categories"
```

### Task 3: Add the custom Tauri titlebar

**Files:**
- Modify: `crates/deepx-tauri/src-tauri/tauri.conf.json`
- Modify: `crates/deepx-tauri/src-tauri/capabilities/default.json`
- Create: `crates/deepx-tauri/src/components/shell/WindowTitlebar.tsx`
- Create: `crates/deepx-tauri/src/components/shell/WindowTitlebar.test.tsx`
- Modify: `crates/deepx-tauri/src/components/shell/AppShell.tsx`
- Modify: `crates/deepx-tauri/src/styles/shell.css`

**Interfaces:**
- Consumes: `getCurrentWindow()` from `@tauri-apps/api/window`.
- Produces: drag region and minimize/maximize/close controls.

- [ ] **Step 1: Write window-control tests with a mocked Tauri window**

```tsx
it("routes titlebar controls to the current Tauri window", () => {
  const h = renderTitlebarWithMockWindow();
  h.click("minimize");
  h.click("maximize");
  h.click("close");
  expect(h.window.minimize).toHaveBeenCalledOnce();
  expect(h.window.toggleMaximize).toHaveBeenCalledOnce();
  expect(h.window.close).toHaveBeenCalledOnce();
});
```

- [ ] **Step 2: Run the test to verify it fails**

```powershell
pnpm --dir crates/deepx-tauri exec vitest run src/components/shell/WindowTitlebar.test.tsx
```

Expected: FAIL because the titlebar does not exist.

- [ ] **Step 3: Implement official Tauri window actions**

Set window configuration:

```json
{
  "title": "DeepX",
  "decorations": false,
  "width": 1200,
  "height": 850,
  "minWidth": 800,
  "minHeight": 600,
  "center": true
}
```

Grant only the official window capabilities required by the titlebar:

```json
{
  "permissions": [
    "core:window:default",
    "core:window:allow-close",
    "core:window:allow-minimize",
    "core:window:allow-toggle-maximize",
    "core:window:allow-start-dragging"
  ]
}
```

Implement controls:

```tsx
const win = getCurrentWindow();
<header class="window-titlebar" data-tauri-drag-region>
  <div class="window-drag-region" data-tauri-drag-region />
  <button aria-label="Minimize" onClick={() => win.minimize()} />
  <button aria-label="Maximize or restore" onClick={() => win.toggleMaximize()} />
  <button aria-label="Close" onClick={() => win.close()} />
</header>
```

Buttons are excluded from the drag region and expose visible keyboard focus.

- [ ] **Step 4: Verify component and Tauri configuration**

```powershell
pnpm --dir crates/deepx-tauri exec vitest run src/components/shell/WindowTitlebar.test.tsx
cargo check -p deepx-tauri
```

Expected: PASS; the Tauri config parses and the component calls official APIs.

- [ ] **Step 5: Commit native window chrome**

```powershell
git add crates/deepx-tauri/src-tauri/tauri.conf.json crates/deepx-tauri/src-tauri/capabilities/default.json crates/deepx-tauri/src/components/shell/WindowTitlebar.tsx crates/deepx-tauri/src/components/shell/WindowTitlebar.test.tsx crates/deepx-tauri/src/components/shell/AppShell.tsx crates/deepx-tauri/src/styles/shell.css
git commit -m "feat(tauri): add custom desktop titlebar"
```

### Task 4: Add full lifecycle and accessibility regression fixtures

**Files:**
- Create: `crates/deepx-tauri/src/test/fixtures/agentLifecycle.ts`
- Create: `crates/deepx-tauri/src/test/fullLifecycle.test.tsx`
- Create: `crates/deepx-tauri/src/dev/visualFixtures.ts`
- Modify: `crates/deepx-tauri/src/App.tsx`

**Interfaces:**
- Consumes: raw reducer, projection, AppShell, permission/ask/plan interactions, and follow-up queue.
- Produces: deterministic complete/error/cancel/waiting fixtures and a development-only visual state selector.

- [ ] **Step 1: Write the end-to-end frontend lifecycle test**

```tsx
it("collapses a completed tool turn and leaves only the approved top-level parts", async () => {
  const h = renderLifecycle(agentLifecycle.successWithExecAndPermission);
  await h.playAll();
  const turn = h.root.querySelector("[data-turn='turn-1']")!;
  expect(turn.querySelector("[data-process-disclosure]")?.getAttribute("aria-expanded")).toBe("false");
  expect(Array.from(turn.children).map(n => n.getAttribute("data-part"))).toEqual([
    "user-prompt", "process", "assistant-answer",
  ]);
});

it("keeps failed and cancelled traces open", async () => {
  expect(await playAndReadExpanded(agentLifecycle.failedTool)).toBe("true");
  expect(await playAndReadExpanded(agentLifecycle.cancelled)).toBe("true");
});
```

- [ ] **Step 2: Run the lifecycle test to verify missing fixture coverage**

```powershell
pnpm --dir crates/deepx-tauri exec vitest run src/test/fullLifecycle.test.tsx
```

Expected: FAIL because fixtures and the lifecycle harness do not exist.

- [ ] **Step 3: Implement deterministic fixtures and visual states**

Export fixtures for:

```ts
export const agentLifecycle = {
  successWithExecAndPermission: [...events],
  failedTool: [...events],
  cancelled: [...events],
  askSingle: [...events],
  askBatch: [...events],
  restoredHistory: [...events],
} as const;
```

In development only, accept `?fixture=completed|running|failed|permission|ask|long-output` and inject fixture state without invoking the agent. Production builds omit the selector through `import.meta.env.DEV` dead-code elimination.

- [ ] **Step 4: Verify lifecycle, keyboard semantics, and build**

```powershell
pnpm --dir crates/deepx-tauri exec vitest run src/test/fullLifecycle.test.tsx
pnpm --dir crates/deepx-tauri run test:run
pnpm --dir crates/deepx-tauri run build
```

Expected: PASS; buttons/disclosures have accessible names and focusable controls.

- [ ] **Step 5: Commit regression fixtures**

```powershell
git add crates/deepx-tauri/src/test crates/deepx-tauri/src/dev/visualFixtures.ts crates/deepx-tauri/src/App.tsx
git commit -m "test(tauri): cover complete conversation lifecycles"
```

### Task 5: Finish responsive themes and visual verification

**Files:**
- Modify: `crates/deepx-tauri/src/styles/tokens.css`
- Modify: `crates/deepx-tauri/src/styles/shell.css`
- Modify: `crates/deepx-tauri/src/styles/conversation.css`
- Modify: `crates/deepx-tauri/src/styles/process.css`
- Modify: `crates/deepx-tauri/src/styles/composer.css`
- Modify: `crates/deepx-tauri/src/styles/interactions.css`
- Create: `docs/superpowers/verification/deepx-tauri-ui-matrix.md`

**Interfaces:**
- Consumes: development fixture selector.
- Produces: reviewed screenshots and a checked verification matrix.

- [ ] **Step 1: Add responsive and reduced-motion assertions**

Add CSS rules with exact breakpoints:

```css
@media (max-width: 1099px) { .task-sidebar { width: 184px; } }
@media (max-width: 799px) {
  .task-sidebar { position: fixed; transform: translateX(-100%); }
  .task-sidebar[data-open="true"] { transform: translateX(0); }
  .environment-popover { inset: auto 12px 12px; width: auto; }
}
@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after { animation-duration: 0.01ms !important; transition-duration: 0.01ms !important; }
}
```

- [ ] **Step 2: Run build before manual visual verification**

```powershell
pnpm --dir crates/deepx-tauri run build
pnpm --dir crates/deepx-tauri run test:run
```

Expected: PASS before screenshots are accepted.

- [ ] **Step 3: Capture and inspect the approved state matrix**

Run the dev app and inspect fixture URLs for completed, running, failed, permission, ask, and long-output states at:

```text
1920x1080 light
1920x1080 dark
1366x768 light
1366x768 dark
800x600 light
800x600 dark
```

Record each state in `deepx-tauri-ui-matrix.md` as `PASS` only after checking no overlap, clipped composer, permanent right rail, ToolCard, or unreadable danger action exists.

- [ ] **Step 4: Run the visual gate after fixes**

```powershell
pnpm --dir crates/deepx-tauri run build
pnpm --dir crates/deepx-tauri run test:run
git diff --check -- crates/deepx-tauri/src docs/superpowers/verification/deepx-tauri-ui-matrix.md
```

Expected: PASS and every matrix row checked.

- [ ] **Step 5: Commit responsive verification**

```powershell
git add crates/deepx-tauri/src/styles docs/superpowers/verification/deepx-tauri-ui-matrix.md
git commit -m "style(tauri): finalize responsive transcript visuals"
```

### Task 6: Delete the legacy UI and comparison flag

**Files:**
- Delete: `crates/deepx-tauri/src/components/MessageItem.tsx`
- Delete: `crates/deepx-tauri/src/components/ThinkingBlock.tsx`
- Delete: `crates/deepx-tauri/src/components/ToolRow.tsx`
- Delete: `crates/deepx-tauri/src/components/MessageList.tsx`
- Delete: `crates/deepx-tauri/src/components/InfoBar.tsx`
- Delete: `crates/deepx-tauri/src/components/StatusPanel.tsx`
- Delete: `crates/deepx-tauri/src/components/InputBar.tsx`
- Delete: `crates/deepx-tauri/src/styles/message-list.css`
- Delete: `crates/deepx-tauri/src/styles/tool-call-card.css`
- Delete: `crates/deepx-tauri/src/styles/info-bar.css`
- Delete: `crates/deepx-tauri/src/styles/status-panel.css`
- Delete: `crates/deepx-tauri/src/styles/input-bar.css`
- Modify: `crates/deepx-tauri/src/components/ChatView.tsx`
- Modify: `crates/deepx-tauri/src/App.tsx`
- Modify: `crates/deepx-tauri/src/main.tsx`
- Create: `crates/deepx-tauri/src/test/legacyAbsence.test.ts`

**Interfaces:**
- Consumes: fully verified new shell and conversation path.
- Produces: one production UI path with no legacy selector or imports.

- [ ] **Step 1: Add a legacy-absence test**

```ts
import { readFileSync, readdirSync } from "node:fs";
import { resolve } from "node:path";

it("contains no legacy renderer imports or selectors", () => {
  const src = resolve(__dirname, "..");
  const files = [
    resolve(src, "App.tsx"),
    resolve(src, "components/ChatView.tsx"),
    resolve(src, "main.tsx"),
    ...readdirSync(resolve(src, "styles"))
      .filter(name => name.endsWith(".css"))
      .map(name => resolve(src, "styles", name)),
  ];
  const source = files.map(path => readFileSync(path, "utf8")).join("\n");
  for (const legacy of ["MessageItem", "ToolRow", "ThinkingBlock", ".tool-card", ".status-panel", ".info-bar", "deepx:new-conversation-ui"]) {
    expect(source).not.toContain(legacy);
  }
});
```

- [ ] **Step 2: Run the test to verify it fails while legacy code remains**

```powershell
pnpm --dir crates/deepx-tauri exec vitest run src/test/legacyAbsence.test.ts
```

Expected: FAIL listing legacy imports/selectors.

- [ ] **Step 3: Remove the old path and imports**

Delete the listed files, remove legacy CSS imports from `main.tsx`, remove the localStorage comparison flag, and render only the new AppShell/ConversationTranscript path. Keep parser utilities only if imported by the new detail renderer.

- [ ] **Step 4: Run the final acceptance gate**

```powershell
pnpm --dir crates/deepx-tauri run build
pnpm --dir crates/deepx-tauri run test:run
cargo check -p deepx-tauri
cargo test -p deepx-tauri
git diff --check
git status --short
```

Expected: all checks PASS; status shows no accidental formatting or unrelated staged changes.

- [ ] **Step 5: Commit final legacy removal**

```powershell
git add crates/deepx-tauri/src crates/deepx-tauri/src-tauri docs/superpowers/verification/deepx-tauri-ui-matrix.md
git commit -m "refactor(tauri): remove the legacy desktop UI"
```
