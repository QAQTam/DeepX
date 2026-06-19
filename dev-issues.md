# DeepX 项目逻辑矛盾与不一致汇总

> 由 5 个子代理分别探索协议层、前端渲染层、消息状态机、配置流、工具执行框架后归中分析得出。

---

## 🔴 跨层关键矛盾 (Critical Cross-Cutting Issues)

### C1. Compact 功能全线失效（3 个模块矛盾）

| 层面 | 问题 | 位置 |
|------|------|------|
| **消息存储** | `to_vec()` 忽略 `compact_skip`，snapshot 保存的是**完整未压缩历史** | `store.rs:473-483` |
| **消息存储** | `compact_skip` 不被序列化，`from_session` 始终重置为 0 | `store.rs:498` |
| **消息循环** | compact 调用用了**硬编码默认 ProviderConfig**，忽略 endpoint 适配字段 | `msglp/src/lib.rs:547-551` |

**结论**: Compact = 一次性操作，保存/重启后即丢失。且 compact 调用对非 OpenAI 提供者（如 Qwen/GLM）发错 URL 路径和参数格式，静默失败。

---

### C2. Balance 余额流全线断裂（4 个模块都不通）

| 层面 | 状态 | 位置 |
|------|------|------|
| **Gate** | 正确从 API 响应解析并发出 `StreamEvent::Balance` | `openai.rs:316-325` |
| **msglp** | `match` 走通配符 `_ => {}`，**直接丢弃** | `msglp/src/lib.rs:773` |
| **协议** | `Agent2Ui::Balance` 定义完好但**从未被 produce** | `agent_protocol.rs:271-276` |
| **前端** | `App.tsx` switch 无 `"balance"` case | `App.tsx:91-121` |

**结论**: Gate 到前端全线断裂，balance 信息在 msglp 层被吞掉。

---

### C3. ToolExecDelta 协议变体是死代码 + 假实现

| 层面 | 状态 | 位置 |
|------|------|------|
| **协议** | 定义了 `Agent2Ui::ToolExecDelta` | `agent_protocol.rs:221-225` |
| **Bridge** | `agent2ui_event_name` 能为它生成事件名 | `bridge.rs:273` |
| **工具框架** | `execute_tools_parallel()` 唯一发送者，但**未被任何代码调用** | `bridge.rs:291` |
| **前端** | `App.tsx` 无 `"tool_exec_delta"` case | `App.tsx:89-121` |

**结论**: 该变体和 `execute_tools_parallel()` 均是死代码，实际执行进度的流走的是 `ExecProgress` 通道。

---

## 🟠 前后端协议矛盾 (Frontend-Backend Protocol Mismatches)

### P1. `RoundDeltaKind::ToolCalling` 被前端静默丢弃

- 后端发送 `kind: "tool_calling"` 的 delta
- 前端 `chat.ts:91-94` 只处理 `"thinking"` 和 `"answering"`，`"tool_calling"` 走不到任何分支
- 数据从协议到渲染全链路丢失

### P2. `Turn.usage` 类型声明字段名与运行时数据不匹配

- TypeScript 类型: `{ input_tokens, output_tokens, total_tokens }`
- 运行时实际数据: `{ prompt_tokens, completion_tokens, total_tokens }`
- `input_tokens` 和 `output_tokens` 永远为 `undefined`
- 目前因为前端不使用 `input_tokens`/`output_tokens` 而未显现，属**潜伏 bug**

### P3. `ToolResultDef.file` 完全未暴露到前端

- 后端 `ToolResultDef` 有 `file: Option<FileSnapshotInfo>`
- TypeScript 接口仅 `{ tool_call_id, output, success }`，缺失 `file` 字段
- 即使后端发送文件快照信息（path/lines/size_bytes），前端结构上无法访问

### P4. `RoundComplete.is_final` 后端发送但前端不接收

- 后端 `RoundComplete` 携带 `is_final: bool`
- 前端 `handleRoundComplete` 签名未包含此参数
- 传输浪费，无实际用途

### P5. `total_tokens` 累加模式不一致

- `handleTurnEnd`: `sessionInfo.totalTokens += u.total_tokens`（累加）
- `handleDashboard`: `sessionInfo.totalTokens = u.total_tokens`（覆盖）
- 两个事件交替触发时 `totalTokens` 值不确定

### P6. `tool_notice` 忽略 `level` 字段

- 后端发送 `{ message, level: "warn"|"error" }`
- 前端只读 `message`，所有 notice 无区别渲染

---

## 🟡 消息状态机问题 (Message Store State Machine)

### M1. 工具线程泄漏（Cancel 时未 join）

**`msglp/src/lib.rs:839-844`**: 取消时 `if cancelled { break }` 跳过 join，已 spawn 的线程继续运行（可能阻塞在 PTY I/O），资源泄漏。

### M2. 工具线程 panic 导致 ToolUse 永久悬空

**`msglp/src/lib.rs:851-853`**: `h.join()` 返回 `Err` 时静默丢弃，不注入错误结果。Step 的 `all_tools_satisfied()` 永远为 false。

### M3. `Effect::CallGate` 死代码

**`effect.rs:10`**: 定义但从未构造。`MessageStore` 只返回 `None` 和 `TurnComplete`。

### M4. 上下文无限增长（无自动截断）

**`store.rs:309-351`**: `build_context_for_gate` 未应用 `context_limit`，会话长度超模型窗口时只能靠手动 compact。

### M5. 流式 delta 残留

**`chat.ts:112-117`**: `if (thinking)` / `if (answer)` 保护模式意味着 RoundComplete 省略某字段时，旧流式 delta 值**永久残留**。

---

## 🟢 工具执行框架问题 (Tool Execution Framework)

### T1. 单工具取消完全失效

- `prepare_req` 为每个工具创建 `cancel_flag: Arc<AtomicBool>`，存入 `inflight_tasks`
- `cancel_tool(Some(id))` 设置对应 flag
- **但没有任何 handler 读取/检查这个 flag**，`ToolCallCtx` 中无此字段

### T2. 并行执行路径的进度流 sender 被立即 drop

**`bridge.rs:254-261`**: `let (ptx, prx) = ...; drop(ptx);` sender 在 handler 运行前就释放，导致 `rx.recv()` 立即返回 `Err(Disconnected)`。

### T3. 16/18 个工具不检查全局 CANCEL

只有 `exec` 和 `explore/scan` 检查 `crate::CANCEL`。`web_fetch`、`search`、`glob`、`grep`、`sed` 等长耗时操作不响应取消。

### T4. 超时机制框架层未实施

`ToolCallCtx.timeout_secs` 和 `handler.default_timeout` 存在但**没有任何执行路径强制执行**。只有 `exec` 自行解析命令参数的 timeout。

### T5. Mutex 中毒静默禁用所有工具

**`bridge.rs:43`**: `.lock().ok()` 将 `PoisonError` 转 `None`，后续工具调用静默失败，日志提示"工具管理器未初始化"而非"mutex 中毒"。

### T6. 安全检查可绕过

- `rm -rf /*` 绕过（字面不匹配）
- Tab 分隔绕过（`sudo\trm -rf`）
- 引号绕过（`"~"`）

---

## 🔵 配置流问题 (Configuration Flow)

### F1. `fetch_models` 完全是死代码

- `registry.rs:227-264` 实现完整但未被任何调用方使用
- TUI 自己简化实现仅验证连通性
- 前端 SettingsView 模型下拉框只显示当前配置的模型

### F2. Compact 调用使用错误 ProviderConfig

**`msglp/src/lib.rs:547-551`**: 所有 endpoint 适配字段（`chat_path`、`thinking_mode`、`cache_field`、`has_balance`等）硬编码为默认值，对非 OpenAI 提供者路径和参数格式均错误。

### F3. API Key 单向屏蔽（无法清除）

- 保存后读回为 `"****"`
- 前端空字符串不触发更新（保存时 `if !api_key.is_empty()` 跳过）
- 用户已保存的 key 不可见也无法清除

### F4. `context7_api_key` 和 `mcp_servers` 无前端 UI

- 后端完全支持这两个字段
- 前端无读取/编辑/保存路径

### F5. 无 Profile 管理 UI

- 后端有完整的 profiles 系统
- 前端 SettingsView 无 profile 切换/管理功能

---

## 📊 总体数据

| 类别 | 数量 | 最严重项 |
|------|------|----------|
| 🔴 跨层关键矛盾 | 3 | Compact 全线失效、Balance 断裂、ToolExecDelta 死代码 |
| 🟠 前后端协议矛盾 | 6 | ToolCalling delta 丢弃、Turn.usage 字段名错误 |
| 🟡 消息状态机 | 5 | 工具线程泄漏、panic 悬空、delta 残留 |
| 🟢 工具执行 | 6 | 单工具取消失效、进度流不工作、超时不实施 |
| 🔵 配置流 | 5 | fetch_models 死代码、compact ProviderConfig 错误 |

### 根因模式

1. **双执行路径**: 工具执行有两条路径（store 的 `execute_tools_batch` vs msglp 的直调），一条完全闲置
2. **快照与结构脱节**: 结构化 `Vec<Turn>` 拍平成 `Vec<Message>` 时丢失 `compact_skip` 等元数据
3. **Option 传播不当**: 前后端 `if (x)` 守卫模式导致空字段时旧数据残留
4. **死代码蔓延**: 协议变体、Effect 变体、函数、函数参数均有死代码未清理
5. **同步 HTTP 无中止**: `ureq` 同步读取无 abort 机制，取消只能"等跑完"
