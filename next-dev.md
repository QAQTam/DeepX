# DeepX Tauri Frontend Migration: React → SolidJS

## 背景

React 在流式输出场景下 CPU 单核 100%：

- **根因**：每个 SSE delta（~50/s）触发全 App 重渲染 + `react-markdown` 全量解析
- **React 瓶颈**：Virtual DOM diff + 组件级重执行 + 重型 Markdown parser
- **SolidJS 优势**：细粒度响应式，信号级 DOM 更新，无 VDOM

## 兼容性调研结论

| 维度 | 结论 | 证据来源 |
| --- | --- | --- |
| **Tauri 官方支持 SolidJS** | ✅ | `create-tauri-app` 内置 SolidJS 模板，可选 TS |
| **Tauri 插件兼容** | ✅ | `@tauri-apps/api` 纯 TS；插件与框架无关 |
| **Vite 构建** | ✅ | `vite-plugin-solid` 替代 `@vitejs/plugin-react` |
| **Tailwind CSS v4** | ✅ | 编译为 vanilla CSS，零冲突 |
| **Markdown 渲染** | ⚠️ 无官方 `solid-markdown` | 用 `marked` (50KB, 零依赖) + `innerHTML` |
| **语法高亮** | ✅ `highlight.js` | 纯 JS，直接可用 |
| **i18n / Bridge / CSS** | ✅ | 纯 TS/CSS，100% 复用 |
| **后端 (Rust)** | ✅ 零改动 | IPC 协议不依赖前端框架 |

## 资产盘点

### 可复用（零改动）

| 文件 | 行数 | 类型 |
|---|---|---|
| `bridge/tauri.ts` | 114 | 纯 TS — Tauri IPC |
| `i18n/en.ts` | 187 | 纯 TS — 英文 |
| `i18n/zh.ts` | 187 | 纯 TS — 中文 |
| `i18n/index.ts` | 44 | 纯 TS — i18n 注册 |
| `types.ts` | 22 | 纯 TS — 类型 |
| `types/agent.ts` | 146 | 纯 TS — 类型 |
| `styles/*.css` | 202 | 纯 CSS |
| `index.css` | 29 | 纯 CSS |

### 需迁移

| 层 | 文件 | 行数 | React 特性使用 |
|---|---|---|---|
| Entry | `main.tsx` | 21 | `createRoot`, `StrictMode` |
| App | `App.tsx` | 421 | `useState`(13), `useEffect`(5), `useCallback`(5), `useRef`(3) |
| Hooks | `useAgent.ts` | 142 | `useReducer`, `useEffect`, `useCallback`, `useRef` |
| Hooks | `useBalance.ts` | 28 | `useState`, `useCallback` |
| Hooks | `useConfig.ts` | 59 | `useState`, `useCallback` |
| Hooks | `useDocuments.ts` | 43 | `useState`, `useCallback` |
| Hooks | `useSession.ts` | 44 | `useState`, `useCallback` |
| Domain | `agent.fsm.ts` | 188 | Reducer pattern (→ `createStore` + `produce`) |
| Domain | `tool-registry.tsx` | 242 | Map-based, `ReactNode` type (→ 删类型即可) |
| Shared | `Button`, `Badge`, `Card`, etc. | ~450 | Props + JSX (→ 几乎直接复制) |
| Shared | `ThemeProvider.tsx` | 67 | `createContext`, `useState`, `useEffect` |
| Shared | `Toast.tsx` | 53 | `createContext`, `useState`, `useCallback` |
| Shared | `ErrorBoundary.tsx` | 49 | Class Component (→ Solid 内置) |
| Biz | `InfoPanel.tsx` | 164 | Props, `useState` |
| Biz | `WorkspacePanel.tsx` | 183 | Props, `useState`, `useEffect` |
| Biz | `SettingsDialog.tsx` | 184 | Props, `useState` |
| Biz | `ConfigWizard.tsx` | 117 | Props, `useState` |
| Biz | `AskUserDialog.tsx` | 68 | Props, `useState` |
| Biz | `StreamIndicator.tsx` | 75 | Props (→ 直接复制) |
| Chat | `ChatMessage.tsx` | 14 | Wrapper (→ 直接复制) |
| Chat | `MessageItem.tsx` | 81 | Props, regex parsing |
| Chat | `ReasoningBlock.tsx` | 41 | `useState`, `useEffect`, `useRef` |
| Chat | `ToolCard.tsx` | 85 | `useState` |
| Chat | `MarkdownBody.tsx` | 67 | `useMemo`, `react-markdown` (→ `marked` + `innerHTML`) |
| **总计** | **~3,400 行** | |

## 关键技术映射

| React | SolidJS | 说明 |
|---|---|---|
| `useState(v)` | `createSignal(v)` | `val` → `val()` |
| `useEffect(fn, [])` | `onMount(fn)` | 挂载时执行 |
| `useEffect(fn, [dep])` | `createEffect(fn)` | 自动跟踪信号 |
| `useCallback(fn, [])` | 直接定义函数 | Solid 无需 useCallback |
| `useRef()` | `let el!: HTMLDivElement` + `ref={el}` | 编译时绑定 |
| `useReducer(r, init)` | `createStore(init)` + `produce` | 复杂状态管理 |
| `useMemo(fn, [dep])` | `createMemo(fn)` | 缓存计算 |
| `createContext` / `useContext` | `createContext` / `useContext` | API 几乎一样 |
| Component class | Solid 内置 `<ErrorBoundary>` | 更简单 |
| `messages.map(...)` | `<For each={messages()}>{(m) => ...}</For>` | 高效列表 |
| `{cond && <X/>}` | `<Show when={cond()}><X/></Show>` | 条件渲染 |
| `react-markdown` | `marked` + `innerHTML` | 50KB vs 1.2MB |

## 包变更

```diff
dependencies:
- react: ^19.2.6
- react-dom: ^19.2.6
- react-markdown: ^10.1.0
- remark-gfm: ^4.0.1
- rehype-highlight: ^7.0.2
+ solid-js: ^1.9.0
+ marked: ^15.0.0

devDependencies:
- @vitejs/plugin-react: ^6.0.1
- @types/react: ^19.2.14
- @types/react-dom: ^19.2.3
- eslint-plugin-react-hooks: ^7.1.1
- eslint-plugin-react-refresh: ^0.5.2
+ vite-plugin-solid: ^2.11.0

Bundle size: -1.5MB
```

## vite.config.ts 变更

```diff
- import react from '@vitejs/plugin-react'
+ import solid from 'vite-plugin-solid'

  export default defineConfig({
-   plugins: [react(), tailwindcss()],
+   plugins: [solid(), tailwindcss()],
    ...
  })
```

## main.tsx 变更

```diff
- import { StrictMode } from 'react'
- import { createRoot } from 'react-dom/client'
+ import { render } from 'solid-js/web'

- createRoot(document.getElementById('root')!).render(
-   <StrictMode>
-     <ThemeProvider>
-       <ToastProvider>
-         <App />
-       </ToastProvider>
-     </ThemeProvider>
-   </StrictMode>,
- )
+ render(() => (
+   <ThemeProvider>
+     <ToastProvider>
+       <App />
+     </ToastProvider>
+   </ThemeProvider>
+ ), document.getElementById('root')!)
```

## MarkdownBody 变更

```diff
- import { useMemo } from 'react'
- import ReactMarkdown from 'react-markdown'
- import remarkGfm from 'remark-gfm'
+ import { marked } from 'marked'
+ import { createMemo } from 'solid-js'

- export function MarkdownBody({ content }: MarkdownBodyProps) {
-   const safeContent = useMemo(() => sanitizeMarkdown(content), [content])
+ export function MarkdownBody(props: MarkdownBodyProps) {
+   const html = createMemo(() => marked(sanitizeMarkdown(props.content)))

    return (
-     <ReactMarkdown remarkPlugins={[remarkGfm]} components={{...}}>
-       {safeContent}
-     </ReactMarkdown>
+     <div class="markdown-body" innerHTML={html()} />
    )
  }
```

## Agent FSM 变更

### React (当前)

```typescript
const [state, dispatch] = useReducer(agentReducer, null, createInitialState)

case 'STREAM_DELTA':
  const stream = { ...state.stream }
  stream.content += action.delta
  return { ...state, stream }
```

### Solid (目标)

```typescript
const [state, setState] = createStore(createInitialState())

function streamDelta(delta: string) {
  setState('stream', 'content', c => c + delta)  // 仅更新 content 字段
}
```

**关键差异**：`setState('stream', 'content', ...)` 只更新 `stream.content` 路径，绑定了该信号的 DOM 节点自动更新，无需全量重渲染。

## 分阶段计划

### Phase 1 — 基础设施 (0.5 天)

- [ ] `pnpm remove` React 依赖，`pnpm add` Solid 依赖
- [ ] 替换 `vite.config.ts` 插件
- [ ] 改写 `main.tsx` 入口
- [ ] 验证 `pnpm tauri dev` 启动空白页
- [ ] 验证 Tauri IPC 正常

### Phase 2 — Shared 组件 (1 天)

- [ ] Button / Badge / Card / EmptyState — 直接复制
- [ ] Spinner / Skeleton / Tooltip — `useState` → `createSignal`
- [ ] Input / Select — `useState` → `createSignal`
- [ ] Toast — `createContext` + `createSignal` + `createStore`
- [ ] ThemeProvider — `createContext` + `createSignal` + `createEffect`
- [ ] ErrorBoundary — 替换为 Solid 内置 `<ErrorBoundary>`
- [ ] 构建 shared/index.ts 导出桶

### Phase 3 — Agent 核心状态 (2 天)

- [ ] `agent.fsm.ts`: reducer → `createStore` + `produce` 修饰器
- [ ] `useAgent.ts`: `useReducer` → `createStore`；`useEffect` → `onMount` + `createEffect`
- [ ] Tauri event listener: `listen()` + `onCleanup`
- [ ] 其他 hooks: `useState` → `createSignal`
- [ ] `types/agent.ts`: `ReactNode` → Solid `JSX.Element`

### Phase 4 — App + 业务组件 (2 天)

- [ ] `App.tsx`: 全部 `useState` → `createSignal`；`useEffect` → `createEffect`
- [ ] `messages.map` → `<For each={messages()}>`
- [ ] 条件渲染 → `<Show when={...}>`
- [ ] InfoPanel / WorkspacePanel / SettingsDialog / ConfigWizard / AskUserDialog
- [ ] MarkdownBody: `react-markdown` → `marked` + `innerHTML`
- [ ] MessageItem / ReasoningBlock / ToolCard / StreamIndicator

### Phase 5 — 流式渲染专项优化 (1 天)

- [ ] `streamContent` 用 `createSignal`，JSX 直接 `{streamContent()}`
- [ ] 流式期间纯文本；流结束后 `marked` 渲染 Markdown
- [ ] 可选 `batch()` 合并多个 delta
- [ ] 性能对比测试 vs React 版本

## 性能预期

| 场景 | React (当前) | Solid (目标) |
|---|---|---|
| 流式 50 deltas/s | CPU 100%（单核） | CPU <5% |
| 打字输入 | CPU 20%（已优化为~0%） | CPU ~0% |
| 消息列表渲染 | 组件树全量 VDOM diff | 仅新增消息 DOM 插入 |
| Markdown 渲染 | react-markdown 1.2MB | marked 50KB, innerHTML |

## 风险与缓解

| 风险 | 缓解 |
|---|---|
| `marked` 功能不如 `react-markdown` | marked 是最流行的 JS parser，支持 GFM/表格/代码高亮 |
| `createStore` 学习曲线 | 仅用于 agent.fsm，其余用 `createSignal` |
| Solid DevTools 生态不成熟 | Chrome 扩展可用，基本调试功能齐全 |
| Tauri + Solid 社区案例少 | Tauri 官方已集成模板，底层无耦合 |
| 迁移期间功能回归 | Phase 3 前保留 React 分支，随时可回退 |

## 时间估算

| Phase | 工时 | 累计 |
|---|---|---|
| P1 基础设施 | 0.5 天 | 0.5 天 |
| P2 Shared 组件 | 1 天 | 1.5 天 |
| P3 Agent 核心 | 2 天 | 3.5 天 |
| P4 App + 业务 | 2 天 | 5.5 天 |
| P5 流式优化 | 1 天 | 6.5 天 |
| **总计** | **~7 天** | |

## 决策

- **后端**：零改动。Bridge/Proto/i18n/CSS 全部复用。
- **推荐策略**：先执行 P1+P2 (1.5 天)，验证 SolidJS 生态满足需求。若通过则继续，若阻塞则回退 React + 流式纯文本修复，损失可控。
