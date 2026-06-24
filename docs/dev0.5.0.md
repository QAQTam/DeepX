# DeepX v0.5.0 开发日志

> 基于 v0.4.0-funny 分支（0.4.0 多 session 修复 + 股市图）

## 一、v0.4.0 收尾（已完成）

### 多 session 并行隔离（10 fixes）

| 层 | 修复 | 文件 |
|----|------|------|
| Backend | `kill_agent` 释放 registry 锁后再 wait 子进程 | `agent_bridge.rs` |
| Backend | `cmd_delete_session` / `cmd_close_session` 锁外等待 | `agent_bridge.rs` |
| Backend | `shutdown_all` drain 实例后锁外等待 | `agent_bridge.rs` |
| Backend | `index.json` IndexLock 文件锁，防跨进程 RMW 竞争 | `store.rs` |
| Backend | 所有写入路径加 `sync_all`（fsync）持久化保护 | `store.rs` |
| Frontend | `hasMore` / `workspace` 全局 signal → ChatStore per-session | `App.tsx`, `chat.ts` |
| Frontend | `session_created` 同步 `activeSeed` + 重映射 `chatStores` | `App.tsx` |
| Frontend | `resumeSession` 把 UI 状态提交移到 `cmd_resume_session` 后 | `App.tsx` |
| Frontend | `init_session` 区分"会话不存在"(Error) vs "数据损坏"(fallback) | `lifecycle.rs` |
| Frontend | `unlistenMap` 替代 `unlistenFns[]`，标签关闭时清理监听器 | `App.tsx` |
| Frontend | `getOrCreateChatStore` pending 去重，防双击重复创建 | `App.tsx` |
| Frontend | ChatView 使用 `props.chat` 而非解构，修复切换 session 不更新 | `ChatView.tsx` |

### CJK 字节切片审计

- 19 UNSAFE + 129 MAYBE → 0 真阳性
- 防御性修复：`agent_bridge.rs:124`, `subagent/src/lib.rs:107`

## 二、股市图 (v0.4.0-funny)

### 数据流

```
write_file / edit_file / delete_file
    │
    ▼
bridge.rs: compute_code_delta() — 从工具参数提取 ±行数
    │
    ▼
msglp/lib.rs: Loop.code_stats 累积 → emit Agent2Ui::CodeDelta (实时)
    │                              → flush_code_stats → code_stats.jsonl (持久)
    ▼
agent_bridge: stdout reader 转发 code_delta → Tauri event
    │
    ▼
前端: App.tsx 累积到 chat.codeDeltas → StockChart.tsx 渲染 K 线
    │
    ▼
InfoBar: 📈 按钮 → 覆盖层弹出 (lightweight-charts v5)
```

### 新增/修改文件

| 文件 | 内容 |
|------|------|
| `proto/agent_protocol.rs` | `Agent2Ui::CodeDelta`, `CodeDeltaRecord`, `CodeDaily` |
| `proto/lib.rs` | 导出新类型 |
| `tools/bridge.rs` | `ToolExecResult.code_delta`, `compute_code_delta()` |
| `msglp/lib.rs` | `Loop.code_stats`, `flush_code_stats()`, `flush_meta_and_stats()` |
| `tauri/agent_bridge.rs` | `cmd_get_code_stats` 命令 + 日期转换 |
| `tauri/lib.rs` | 注册命令 |
| `store/chat.ts` | `CodeDelta` 类型, `codeDeltas` signal |
| `StockChart.tsx` | K 线 + 成交量柱 (lightweight-charts v5) |
| `InfoBar.tsx` | 📈 按钮 + 覆盖层面板 |
| `ChatView.tsx` | 传 `codeDeltas` |
| `App.tsx` | `code_delta` 事件处理 |
| `info-bar.css` | 覆盖层 + 面板样式 |

### 数据源

| 工具 | 记录内容 |
|------|---------|
| `write_file` | lines_added = 新内容行数, files_created = 1 |
| `edit_file` | lines_added = new_string 行数, lines_removed = old_string 行数 |
| `delete_file` | files_deleted = 1 |

### 已知缺口

- `move_file` / `linuxmod sed` / `edit_file_diff` 未挂钩
- 图表打开后不实时更新（delta 入了 signal 但没推给图表）
- `file` 字段存了但未用于热力图

## 三、v0.5.0 规划

### P0 架构修复（阻塞产品质量）

1. **AgentBridge 重构** — `OnceLock<Mutex<AgentRegistry>>` → per-agent actor
2. **多文件协同编辑** — agent 可规划并执行跨文件重构
3. **Diff 审批** — 工具产生 diff → 展示 → 用户 accept/reject
4. **Agent 任务规划器** — 用户需求 → 文件编辑计划 → 分步验证

### P1 差异化

5. **Session Tabs** — 多 session 并列运行，拖拽消息跨 session
6. **Headless CI** — `deepx --ci "fix all clippy warnings"`
7. **代码库语义索引** — explore 升级到符号级
8. **Git 工作流集成** — 原生 PR、commit message、branch diff
9. **模型智能路由** — 简单任务用便宜模型

### P2 独特体验

10. **Agent Zoo** — 可视化所有运行的 subagent
11. **Bug Bounty 模式** — 并行 subagent 找 bug → 计分
12. **Session Fork** — 从任意 turn 分叉新 session
13. **Achievement 系统** — 勋章成就

### P3 月球项目

14. **Self-Healing Codebase** — 持续扫描卫生/安全/性能，自动提 PR
15. **AI 根因调试** — 崩溃日志 → 源码追踪 → 修复建议
16. **Spectate Mode** — 实时观看另一个 session
17. **离线混合模型** — 本地小模型 + 云端大模型

## 四、架构演进方向

### Agent 间通信

当前架构已天然支持，缺的只是一个消息路由层（~32 行）：

```
Agent A emit InterAgentMessage { target: "B", payload }
    → bridge stdout reader 识别
    → send_to_agent("B", Ui2Agent::InterAgentMessage { from: "A", payload })
    → Agent B dispatch 处理
```

### Subagent 开会模式

同样可行，subagent 需要改为常驻（不退出的 loop），parent agent 作为主持人路由消息。

### JSONL 轻数据库

已具备 ACID 级别可靠性：
- 原子追加: `O_APPEND` + `sync_all`
- 原子替换: `tmp+rename` + `sync_all`
- 跨进程索引锁: `IndexLock` 指数退避
- 读修复: parse 失败跳过 + 下次 save_full 清理

无需切换 SQLite，直到出现跨记录聚合查询需求。

## 五、技术债务清单

| 项 | 优先级 | 说明 |
|----|--------|------|
| Seed newtype | P1 | `SessionSeed(String)` 含验证 |
| 存储 schema 版本 | P1 | `meta.json` 加 `schema_version` |
| Config payload | P1 | `cmd_save_config` 18 参数 → JSON |
| error 传播 | P1 | persistence 失败 → Error 事件 |
| handle_user_input 拆分 | P2 | 300 行方法 → plan/execute/verify |
| 常量集中 | P2 | flush interval / backoff 统一管理 |
