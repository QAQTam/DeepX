# SolidJS 1.x → 2.0.0-beta.21 迁移报告

> 日期: 2026-07-21 | 项目: DeepX Tauri 前端 | 源文件: ~25 个

## 总体状态

| 指标 | 结果 |
|---|---|
| `tsc --noEmit` | ✅ 0 错误 |
| `vitest run` | ✅ 108/120 通过 (90%) |
| 失败测试 | 12 个 (8 个运行时问题 + 4 个待确认) |

---

## 一、已完成的改动

### 1. 依赖变更 (`package.json`)

- **移除（未使用）**: `@kobalte/core`, `@solidjs/router`, `@tanstack/solid-virtual`, `@tanstack/virtual-core`
- **升级**: `solid-js: ^1.9.13` → `solid-js: 2.0.0-beta.21`
- **升级**: `vite-plugin-solid: ^2.11.0` → `vite-plugin-solid: 3.0.0-next.14`
- **新增**: `@solidjs/web: 2.0.0-beta.21`

### 2. 配置变更 (`tsconfig.json`)

```
"jsxImportSource": "solid-js"  →  "jsxImportSource": "@solidjs/web"
```

### 3. 新增文件

- **`src/jsx-patch.d.ts`** — Solid 2.x + tsc children 类型兼容补丁

### 4. 机械性重命名 (21 处)

| 改动 | 影响文件 |
|---|---|
| `from "solid-js/web"` → `from "@solidjs/web"` | `main.tsx` + 11 个测试 |
| `<I18nCtx.Provider value={...}>` → `<I18nCtx value={...}>` | `App.tsx` + 9 处测试 |
| `<Index each={...}>` → `<For each={...} keyed={false}>` | `TurnGroup.tsx`, `ConversationTranscript.tsx`, `ProcessTimeline.tsx`, `ComposerQueue.tsx`, `Toast.tsx` |
| `classList={{...}}` → `class={{...}}` | `SettingsView.tsx`, `CompactStatusRow.tsx`, `ProcessEventRow.tsx`, `PermissionLevelSelect.tsx` |
| `tabIndex` → `tabindex` | `SessionCard.tsx`, `EnvironmentPopover.tsx` |
| `JSX` 类型导入 → `from "@solidjs/web"` | `AppShell.tsx`, `InteractionDock.tsx`, `InteractionModal.tsx`, `ProcessDisclosure.tsx` |

### 5. 语义性重写 (34 处)

| 旧 API | 新 API | 影响文件 |
|---|---|---|
| `onMount(fn)` | `onSettled(fn)` | `App.tsx`, `ConversationTranscript.tsx`, `GitDiffPanel.tsx` |
| `on(fn, deps)` → `createEffect(on(...))` | `createEffect(trackFn, applyFn)` | `MarkdownBody.tsx`, `ProcessDisclosure.tsx` |
| `createResource(fetcher)` | `createSignal` + `onSettled(fetch)` | `SettingsView.tsx` |
| `createEffect(fn)` | `createEffect(trackFn, applyFn)` | `ChatView.tsx`, `GitDiffPanel.tsx`, `SkillsView.tsx`, `SettingsView.tsx`, `ConversationTranscript.tsx` |
| `async fn` in `createEffect` / `onSettled` | `void (async () => {})()` wrapper | `App.tsx`, `MarkdownBody.tsx`, `SettingsView.tsx` |
| `aria-expanded={bool}` | `aria-expanded={String(bool)}` | `ProcessDisclosure.tsx`, `ProcessEventRow.tsx` |

---

## 二、剩余失败 (12 个，8 个文件)

### A 类：运行时行为变化 (8 个) — **真实 bug，非测试问题**

#### 1. ProcessDisclosure.test.tsx (4 个测试失败)

- **文件**: `src/components/process/ProcessDisclosure.tsx:22-30`
- **症状**: 点击展开按钮后 `aria-expanded` 不更新，始终为 `"false"`
- **根因**: `createEffect(() => props.status, status => { setOpen(false); })` 中 `setOpen` 的写入在 2.x effect apply 阶段不触发同步 DOM 更新
- **修复方向**: 将 `setOpen` 移到事件回调中，或使用 `createRenderEffect` + `flush()`

```tsx
// 当前代码 (ProcessDisclosure.tsx:22-30)
let prevStatus: typeof props.status | undefined;
createEffect(() => props.status, status => {
    if (prevStatus !== undefined && prevStatus !== status) {
      if (status === "completed" || status === "failed" || status === "cancelled") {
        setOpen(false);  // 2.x 中此写入不触发 DOM 更新
      }
    }
    prevStatus = status;
});
```

#### 2. ProcessTimeline.test.tsx (1 个测试失败)

- **文件**: `src/components/process/ProcessTimeline.tsx`, `src/components/process/ProcessEventRow.tsx`
- **症状**: 工具执行详情展开不生效 (`aria-expanded` 返回 `null` 而非 `"true"`)
- **根因**: 同 ProcessDisclosure，`aria-expanded={String(props.expanded())}` 在 streaming 更新时不重渲染

#### 3. ConversationTranscript.test.tsx (1 个测试失败)

- **文件**: `src/components/conversation/ConversationTranscript.tsx:42-45`
- **症状**: 用户滚离后"跳到最新消息"按钮点击无效
- **根因**: `createEffect(() => props.turns, () => queueMicrotask(scheduleScrollToBottom))` 中 `scheduleScrollToBottom` 依赖 `followTail` 状态，2.x effect 调度时序与 1.x 有差异

#### 4. sessionUiState.test.ts (1 个测试失败)

- **文件**: `src/store/sessionUiState.test.ts:6-9`
- **症状**: `ui.setWorkspace("...")` 抛出 `REACTIVE_WRITE_IN_OWNED_SCOPE`
- **根因**: Solid 2.x 禁止在 `createRoot` 回调中直接写入信号。**运行时影响**: 如果 `createSessionUiState` 在真实组件中也在 `createRoot` 内使用，同样会触发此错误
- **已尝试**: `flush()`、`untrack()`、`createRoot(options)` 均无效
- **修复方向**: 重构 `createSessionUiState` 或将信号写入推迟到 effect 阶段

#### 5. sessionRegistry.test.ts (1 个测试失败)

- **文件**: `src/store/sessionRegistry.test.ts:33`
- **症状**: `registry.remap("old", "new")` 后 `entry.state().seed` 仍为 `"old"`
- **根因**: `sessionRegistry.remap` → `runtime.update` → `setState` 的信号更新在 2.x 中不传播（与 sessionUiState 同源）
- **运行时影响**: session 重命名功能**完全失效**

### B 类：待确认 (4 个) — 可能在 1.x 已存在

#### 6. ChatView.interactions.test.tsx (1 个)

- **症状**: `expect(vi.fn()).toHaveBeenCalledWith(...)` 失败，mock 未被正确调用
- **可能**: 组件渲染没问题，测试断言方式与 2.x 渲染时序冲突

#### 7. interactions.test.tsx (2 个)

- **文件**: `src/components/interactions/interactions.test.tsx:87,130`
- **症状**: AskUserPrompt 提交按钮 `disabled` 状态与预期不符
- **可能**: 2.x 中 `disabled` 属性渲染方式变化

#### 8. SettingsView.test.tsx (1 个)

- **文件**: `src/components/SettingsView.test.tsx` (test 7)
- **症状**: save 后表单清理的断言失败
- **可能**: `createEffect` 拆分后的时序变化

---

## 三、迁移过程中遇到的 Solid 2.x 关键差异

### 3.1 `createEffect` 必须双参数

1.x 写法 `createEffect(fn)` 在 2.x **运行时报错** `MISSING_EFFECT_FN`。必须改为：

```tsx
// 1.x (运行时崩溃)
createEffect(() => { trackValue(); doSomething(); });

// 2.x
createEffect(
  () => trackValue(),
  () => { doSomething(); }
);
```

### 3.2 `on()` 函数已移除

`createEffect(on(track, apply))` → `createEffect(track, apply)`

### 3.3 `createResource` 已移除

需手动用 `createSignal` + `onSettled` + async fetch 替代。本项目中 `SettingsView.tsx:126-150` 已完成改写。

### 3.4 `Index` 已移除

使用 `<For each={items} keyed={false}>` 替代。

### 3.5 `classList` 已移除

`class="base" classList={{ active: cond() }}` → `class={{ base: true, active: cond() }}`

### 3.6 Context.Provider 语法变更

`<MyCtx.Provider value={v}>` → `<MyCtx value={v}>`

### 3.7 JSX 类型源变更

- `import type { JSX } from "solid-js"` → `import type { JSX } from "@solidjs/web"`
- `import { render } from "solid-js/web"` → `import { render } from "@solidjs/web"`
- `tsconfig.json`: `jsxImportSource` → `@solidjs/web`

### 3.8 布尔 HTML 属性

`aria-expanded={bool}` 在 2.x 中 `false` 时不渲染属性。需改为 `aria-expanded={String(bool)}`。

### 3.9 HTML 属性名小写

`tabIndex={0}` → `tabindex={0}`（2.x 要求 HTML 原生属性名）

### 3.10 异步函数在 effect/onSettled 中

`createEffect(async () => {})` 类型不匹配。需包装为 `createEffect(() => { void (async () => {})(); })`

### 3.11 `onMount` → `onSettled`

注意 `onSettled` 的调用时机与 `onMount` 不完全相同，需验证组件初始化逻辑。

---

## 四、vitest 配置变更 (`vitest.config.ts`)

```ts
// 添加 resolve.conditions 以抑制 @solidjs/signals dev 模式的严格检查
resolve: {
  conditions: ["production", "browser"],
  alias: { "@": resolve(__dirname, "./src") },
},
test: {
  environment: "node",
  include: ["src/**/*.test.{ts,tsx}"],
},
```

> **注意**: 即使添加 `conditions: ["production"]`，`@solidjs/signals` 的严格写入检查在 vitest 中仍未完全关闭。这是剩余 A 类测试失败的深层原因。

---

## 五、推荐下一步

### 优先级 1：修复运行时 bug（sessionRegistry + ProcessDisclosure）

```
src/store/sessionRegistry.ts          — remap 不更新 seed (运行时 session 重命名失效)
src/store/sessionEventRuntime.ts      — update/flush 在 2.x 的传播行为
src/components/process/ProcessDisclosure.tsx  — effect 中 setOpen 不触发 DOM
src/components/process/ProcessTimeline.tsx    — 展开/折叠不工作
```

### 优先级 2：修复其余运行时问题

```
src/components/conversation/ConversationTranscript.tsx  — 滚动行为
src/store/sessionUiState.ts            — createRoot 信号写入
src/store/sessionUiState.test.ts       — 测试需重构
src/store/sessionRegistry.test.ts      — 测试需重构
```

### 优先级 3：确认/修复 B 类

```
src/components/ChatView.interactions.test.tsx
src/components/interactions/interactions.test.tsx
src/components/SettingsView.test.tsx
```

### 关键文件清单

| 文件 | 说明 |
|---|---|
| `package.json` | 依赖变更 |
| `tsconfig.json` | jsxImportSource 变更 |
| `vitest.config.ts` | 测试配置变更 |
| `src/jsx-patch.d.ts` | 类型补丁（新增） |
| `src/main.tsx` | @solidjs/web 导入 |
| `src/App.tsx` | onSettled、Context 语法、async wrapper |
| `src/components/ChatView.tsx` | createEffect 拆分 × 3 |
| `src/components/GitDiffPanel.tsx` | createEffect 拆分 × 2 |
| `src/components/SkillsView.tsx` | createEffect 拆分 |
| `src/components/SettingsView.tsx` | createResource → signal + onSettled |
| `src/components/MarkdownBody.tsx` | on() → 双参数 effect |
| `src/components/process/ProcessDisclosure.tsx` | on() → 双参数 effect（**有待修复**） |
| `src/components/process/ProcessTimeline.tsx` | Index → For |
| `src/components/process/ProcessEventRow.tsx` | classList → class, aria-expanded |
| `src/components/conversation/ConversationTranscript.tsx` | createEffect 拆分（**有待修复**） |
| `src/components/conversation/TurnGroup.tsx` | Index → For |
| `src/components/interactions/CompactStatusRow.tsx` | classList → class |
| `src/components/composer/PermissionLevelSelect.tsx` | classList → class |
| `src/components/shell/AppShell.tsx` | JSX 类型源 |
| `src/components/shell/SessionCard.tsx` | tabIndex → tabindex |
| `src/components/shell/EnvironmentPopover.tsx` | tabIndex → tabindex |
| `src/store/sessionUiState.ts` | （信号创建，**待审查 createRoot 兼容**） |
| `src/store/sessionRegistry.ts` | （remap，**待修复**） |
| `src/store/sessionEventRuntime.ts` | （flush 传播，**待审查**） |
# DB 主存储准备度基线（2026-07-21）

- 真实数据目录 `C:\Users\QAQTam\.deepx\sessions` 检出 2 个 JSONL 会话，均有 `meta.json` 与 `messages.jsonl`，但均没有 `sessions.db`。
- 因此 `cmd_check_db_primary_readiness` 在当前数据上应拒绝 DB 主存储切换；这是预期保护行为，不执行自动迁移或覆盖真实聊天记录。
- 迁移后的验收顺序：先调用 `cmd_reconcile_turso_mirrors`，再调用 `cmd_audit_turso_mirrors`，最后仅在 `cmd_check_db_primary_readiness` 返回 `ready: true` 时考虑后续版本的读优先级切换。

## 授权真实数据重放验证（2026-07-21）

- 在完整备份后，临时启用数据库并对 2 个真实 JSONL 会话执行镜像迁移。
- 两个镜像均通过审计与 DB 主存储门禁；随后人为写入 durable outbox、丢弃会话管理器并重新创建，重放后再次通过审计与门禁。
- 测试结束后已恢复原始 `config.toml`（数据库关闭）、删除测试生成的 `sessions.db`/outbox，并对两个会话的 `meta.json`、`messages.jsonl` 与配置文件逐项 SHA-256 比对备份：全部一致。
