---
name: deepx-arch
description: DeepX Rust 项目架构决策与重构指导。当 qaqtam 遇到以下任何情况时必须使用此 skill：新增字段/事件/协议、重构 runner 或 gate 层、讨论模块边界、发现重复代码（如多处相同字段赋值）、设计 agent struct 字段、决定数据应走事件流还是存 struct、任何涉及 dsx-proto / dsx-agent / runner 的架构变动。也覆盖"我感觉哪里不对但说不清楚"类的架构直觉问题。
---

# DeepX Architecture Skill

## 项目层级地图

```
dsx-types          ← 纯数据类型，无逻辑
dsx-session        ← 会话持久化，SHA-256 checksum，JSON
dsx-proto          ← 进程间协议（Ui2Agent 枚举 + 所有事件 struct）
dsx-tools          ← 工具实现（14 个文件），无 agent 依赖
dsx-agent          ← 核心逻辑（见下方内部地图）
  ├─ gate/         ← Provider 抽象层：SSE 解析、StreamEvent
  ├─ agent/        ← AgentState struct 及子模块
  ├─ assembly.rs   ← Context 组装（消息历史 → API payload）
  ├─ runner/       ← 执行引擎（见下方）
  ├─ orchestrator/ ← 跨轮次维护（file tracking、learning）
  └─ tool_parser.rs ← DSML/XML 工具调用解析
dsx-tui            ← TUI 前端，消费 dsx-proto 事件
src-tauri          ← Tauri 前端桥接，消费 dsx-proto 事件
```

### runner/ 内部职责分工

| 文件 | 职责 | 不该做的 |
|------|------|----------|
| `mod.rs` | 主循环 `run_agent_loop`，分发 Ui2Agent 消息 | 业务逻辑 |
| `lifecycle.rs` | session init/create | token 计算 |
| `api_turn.rs` | 单次 API 调用，捕获 SSE，返回 `(content, reasoning, tcs, usage, stop_reason)` | 修改 AgentState |
| `turn.rs` | 处理一轮完整 turn：调用 api_turn → 处理工具 → 发射事件 | 直接调 gate |
| `ui_emit.rs` | 构建 UI 消息体（assistant/tool display） | 持有 agent 状态 |

---

## 数据所有权规则

### AgentState 字段准入标准

字段放入 `AgentState` **当且仅当**满足以下条件之一：
- 需要**跨多个 turn** 累加或追踪（如 `session.tokens`）
- 是**配置**（`config`，从 TOML 来）
- 是**会话标识**（session seed、session id）
- 是**文件追踪状态**（FileTracker）

❌ **不该放 AgentState 的**：
- 单次 turn 的临时结果（用局部变量或返回值）
- 从另一个字段可派生的值（如 `token_estimate` = `api_usage.prompt_tokens`）
- 只用于立即发射一个事件的中间值

### 当前已知违规
- `agent.token_estimate: u32` — 是 `api_usage.prompt_tokens` 的冗余拷贝，应删除
- `agent.api_usage: Option<UsageInfo>` — 只用于 Dashboard 发射，考虑直接从 `api_turn` 返回值传递

---

## 事件协议决策树

```
新数据需要发给前端？
│
├─ 是轮次级别（每次 API 调用产生一次）？
│   ├─ 是 token 用量 / cache 数据 → Dashboard.usage: Option<UsageInfo>
│   ├─ 是轮次结束状态（stop_reason）→ TurnEnd
│   └─ 是新的独立数据域 → 考虑新事件
│
├─ 是 session 级别（跨轮次累积）？
│   ├─ session_tokens（累计）→ Dashboard.session_tokens 或 TurnEnd 后前端自己累加
│   └─ 其他累计指标 → Dashboard
│
└─ 是实时流数据（token by token）？→ Delta 事件（现有 AssistantDelta）
```

### Dashboard vs TurnEnd 分工原则

| | Dashboard | TurnEnd |
|---|---|---|
| 触发时机 | 每次有新 usage 数据时 | 每个 turn 结束时 |
| 数据性质 | **瞬时快照**（当前 context 状态） | **轮次总结**（stop reason、turn id） |
| token 数据 | ✅ 全部在这里 | ❌ 不重复 |
| context_limit | ✅（配置值，快照需要） | ❌ |
| session_tokens | ✅（累计到现在） | ❌ |

**规则**：token 相关字段单写在 Dashboard。TurnEnd 只关心"这轮怎么结束的"。

---

## 重构前检查清单

改任何 dsx-proto 事件或 AgentState 字段前，回答：

1. **溯源**：这个数据从哪里产生？（`api_turn` 返回值 → `turn.rs` → 事件）
2. **唯一性**：这份数据在 codebase 里有几份拷贝？（> 1 就是问题）
3. **生命周期**：需要活过当前 turn 吗？（否 → 不进 AgentState）
4. **消费方**：前端只在一个地方用，还是多处？（多处 → 考虑是否该拆事件）
5. **影响面**：改 agent_protocol.rs 之后，哪些文件要跟着改？（`grep -rn "StructName"` 先）

---

## 当前待处理的已知问题

### 高优先级
- [ ] `tool` role 修复：工具结果必须用 `role: "tool"`，不能用 `role: "user"`，否则破坏 thinking trace
- [ ] `token_estimate` 冗余字段消除

### 中优先级  
- [ ] Dashboard 重构：加 `usage: Option<UsageInfo>` + `context_limit`，删散字段
- [ ] TurnEnd 瘦身：删 `context_tokens` / `context_limit` / `session_tokens`
- [ ] `cache_tokens()` 函数删除（thin wrapper 无存在价值）

### 低优先级
- [ ] `web.rs` Bing scraping（在 dsx-tools）标注为脆弱，待替换

---

## 模块边界红线

以下依赖方向**不能反转**：

```
dsx-tools → dsx-agent     ❌ 工具不能知道 agent 存在
gate/ → runner/           ❌ gate 只负责 HTTP/SSE，不发射 UI 事件
assembly.rs → turn.rs     ❌ 组装层不依赖执行层
```

新代码放错层的信号：
- 在 `gate/` 里出现 `AgentState` → 错
- 在 `turn.rs` 里直接构建 HTTP 请求 → 错
- 在 `assembly.rs` 里累加 session tokens → 错

---

## 新增功能接线流程

1. **数据从哪来** → 确认是 gate SSE、用户输入、还是工具结果
2. **放哪个 struct** → 按上方准入标准判断
3. **走哪个事件** → 按决策树判断
4. **改 agent_protocol.rs** → 先改协议，再改实现
5. **grep 影响面** → `grep -rn "EventName\|field_name" crates/`
6. **前端 handler** → handleDashboard / handleTurnEnd 对应更新

---

## 上下文限制

- warm zone 上限：128K tokens（V4 attention 可靠区间）
- 工具描述：~13K tokens，是压缩优先级最高的
- prompt.rs 1-3 句限制与 debug 分析冲突 → 用条件规则（debug 场景放宽）
