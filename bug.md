# Bug: StartupView flashes / auto-resumes to latest session

## 症状

启动 DeepX Tauri 应用后，设计的 StartupView（开屏页）仅显示约 2 秒，随后自动进入最近一次会话（ChatView），而非等待用户主动选择"新对话"或"恢复历史会话"。

## 预期行为

每次启动默认显示 StartupView：
- 居中输入框，输入即创建新会话
- 下方显示最近 5 个历史会话卡片，点击恢复
- 左侧 sidebar 保留完整 session list
- 用户不操作则始终停留在 StartupView

## 实际行为

启动后 StartupView 出现约 2 秒，随后自动切换到 ChatView 并进入最新 session。

## 已排查的路径（均排除）

### 前端 (Tauri SolidJS)

1. **`onMount` 自动恢复** — 已移除 `cmd_resume_session` 和 `cmd_new_session` 调用，仅加载 session list。
2. **`ready` 事件自动创建** — 已改为 no-op，不再发送任何 agent 命令。
3. **`Dashboard` 事件设置 seed** — `emit_dashboard()` 发送 `session_seed: ""`，空字符串在 JS 中为 falsy，`handleDashboard` 的 `if (data.session_seed)` 不执行。
4. **localStorage 残留 seed** — `onMount` 首行主动 `localStorage.removeItem(LS_KEY)` 清除。
5. **`hasChosenSession` 信号加固** — 新增独立信号控制 StartupView 可见性，即使 `sessionInfo.seed` 被意外设置也不切换。

### 后端 (Rust Agent)

1. **Agent 启动不自动建 session** — `Loop::run()` 仅发送 `Dashboard` + `Ready`，不发送 `SessionCreated`/`SessionRestored`。
2. **`--resume-seed` CLI 参数** — Tauri `AgentBridge::init` 只传 `agent` 参数，不传 `--resume-seed`。
3. **`.active_session` 文件** — 无代码在启动时读取该文件并自动 resume。
4. **`handle_user_input` 自动创建** — 仅在收到 `UserInput` 且 seed 为空时触发，不会在无用户交互时自发执行。

### Tauri Bridge (Rust)

1. **`AgentBridge::init`** — 仅 spawn agent 子进程 + 设置事件监听，不发送任何 `Ui2Agent` 命令。
2. **`cmd_list_sessions`** — 直接读 `SessionManager::global().list()`，不经过 agent IPC。

## 相关文件

| 文件 | 关键行 | 说明 |
|------|--------|------|
| `crates/deepx-tauri/src/App.tsx` | 88-140 | `onMount` 启动逻辑 |
| `crates/deepx-tauri/src/App.tsx` | 104-112 | `ready` 事件处理 |
| `crates/deepx-tauri/src/App.tsx` | 27,213 | `hasChosenSession` 信号 + 渲染条件 |
| `crates/deepx-tauri/src/components/StartupView.tsx` | 全文件 | 开屏页组件 |
| `crates/deepx-tauri/src-tauri/src/agent_bridge.rs` | 41-125 | AgentBridge::init |
| `crates/deepx-msglp/src/lib.rs` | 291-335 | `Loop::run()` 启动流程 |
| `crates/deepx-msglp/src/lib.rs` | 593-606 | `handle_user_input` 自动创建 |
| `crates/deepx-msglp/src/agent.rs` | 20-22 | `AgentState::new` 初始 seed |
| `crates/deepx-msglp/src/lifecycle.rs` | 13-79 | `init_session` / `create_session` |
| `crates/deepx-session/src/manager.rs` | 28-50 | `SessionManager::init` / `list` |
| `crates/deepx-message/src/store.rs` | 187-213 | `save_msg` / `flush_meta` 空 seed 保护 |

## 尝试的修复

1. `agent.rs:23` — 初始 seed 从 `"init"` 改为 `""`
2. `store.rs:198-200` — `flush_meta` / `snapshot_full` 增加空 seed 保护
3. `App.tsx:88-92` — `onMount` 清除 localStorage seed
4. `App.tsx:104-112` — `ready` 事件改为 no-op
5. `App.tsx:139-144` — 移除 `onMount` 中的 auto-resume/auto-create
6. `App.tsx:27,213` — 新增 `hasChosenSession` 信号解耦

## 可能的未排查方向

- Tauri 事件系统是否在 dev 模式下重放/缓存事件
- Tauri `setup` hook 或 plugin 是否有隐式的 agent 命令
- SolidJS 响应式更新是否触发了意外的 `Show` 组件重渲染
- `agent-event` listener 是否在注册前就收到了被 Tauri 缓存的事件
- `session_restored` 是否通过某种路径在 agent 启动时被意外触发
- 前端 hot-reload (HMR) 是否导致组件状态残留
