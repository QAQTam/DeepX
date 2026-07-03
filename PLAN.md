# DeepX — Task & User Back Message Enhancement

## 背景

`ask_user` 工具通过 `[USER_QUERY] {...}` 标记实现 agent → 前端 → 用户 → agent 的闭环：
- 工具返回 `[USER_QUERY] {"question":"...", "options":[...]}`
- 前端 `handleToolResults` 截获 → AskDialog
- 用户回答 → `cmd_send_message` → UserInput 帧发回 agent

现扩展三个 task 交互功能。

---

## Feature A: 用户取消 Task

**触发**：StatusPanel 中 pending/in_progress task 旁出现 "✕" 按钮

**流程**：
1. 前端调用新 Tauri 命令 `cmd_task_action(seed, "cancel", taskId: u32)`
2. Rust 后端直接修改 `sessions/{seed}/tasks-mem.md`（更新状态行）
3. 同时向 agent 发送 `Ui2Agent::ToolCall { name:"task", action:"update", args:{id, status:"cancelled"} }`
4. Agent 处理 tool call → 更新内存 → emit Dashboard → 前端刷新

**风险**：低。工具调用在 agent 命令通道排队，busy 状态也不丢帧。

---

## Feature B: 用户删除 Task

**触发**：StatusPanel 中已完成/已取消 task 旁出现 "🗑" 按钮

**流程**：
1. `cmd_task_action(seed, "delete", taskId)`
2. 从 `tasks-mem.md` 删除对应行
3. 向 agent 发 `Ui2Agent::ToolCall { name:"task", action:"delete", args:{id} }`

**风险**：低。

---

## Feature C: 询问 Task

**触发**：StatusPanel 中每个 task 旁出现 "?" 按钮

**流程**：
1. 前端直接 `cmd_send_message { seed, text: "Task T{id}: {subject}. 请详细说明实现方案和当前进度。" }`
2. Agent 作为普通 UserInput 处理 → LLM 读取 task 上下文 → 生成回答

**风险**：无（纯前端便利函数，不需新后端）。

---

## 需新增/修改

### Rust 后端（`agent_bridge.rs`）
- 新增 `cmd_task_action(seed, action, taskId)` — 读/写 tasks-mem.md + 发送 ToolCall 到 agent

### 前端 TSX
- `StatusPanel.tsx` — 每个 task row 加 3 个按钮（cancel / delete / ask）
- `chat.ts` — 暴露 `submitTaskAction` 方法

### 前端 CSS
- `status-panel.css` — task 按钮样式

---

## 执行顺序

1. `cmd_task_action` Rust 实现
2. 注册 Tauri 命令
3. StatusPanel 按钮 + 样式
4. chat store 集成
5. 测试 agent busy 时 task 更新是否排队正确
