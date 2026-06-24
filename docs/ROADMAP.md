# DeepX v0.4 → v0.5 Roadmap

> 基于架构审查、竞争分析和创意头脑风暴生成。2025-07。

## 一、现状评估

### 坚实的地基 ✅

| 决策 | 为何能扩展 |
|------|-----------|
| **每 session 独立 OS 进程** | 故障隔离、独立生命周期、无共享可变状态 |
| **Channel 事件循环 + 中断优先级** | Cancel 在流式门控期间也能生效，很多系统做错 |
| **双层 CancelToken** | 读取线程直接写原子标志，绕过 channel 队列 |
| **tmp+rename 原子写入** | 防部分写入损坏，正确性标杆 |
| **IndexLock 指数退避锁** | 跨进程索引协调，降级不挂死 |
| **前端 ChatStore Map 按 seed 去重** | 防快速切换竞态 |

### 脆弱点 🔴

| 问题 | 位置 | 风险 |
|------|------|------|
| **全局 Mutex 瓶颈** | `agent_bridge.rs:31` — `OnceLock<Mutex<AgentRegistry>>` | agent 数增长后所有命令排队 |
| **IndexLock TOCTOU 竞争** | `store.rs:200` — 两个进程可能同时通过锁 | 低概率但存在索引丢失 |
| **IPC channel 容量无监控** | `lib.rs:117` — `sync_channel(256)` | 饱和时回压到前端 UI 线程 |
| **cmd_save_config 18 参数** | `agent_bridge.rs:352` | 每次加配置字段改 4 处 |
| **错误吞噬** | `manager.rs:107,144` — `log::error!` + return | 前端看到成功 UI，实际未保存 |

### 技术债 🟡

- `handle_user_input` 300 行巨型方法
- Session seed 是裸 `String`，无 newtype 验证
- 无存储 schema 版本号，迁移是一次性的
- `code_delta` 事件处理在 App.tsx 中重复注册（已修复于 0.4.0-funny）
- 硬编码常量散落各处（flush interval / backoff / kill timeout）

---

## 二、竞争格局

### 桌面上必须有的（缺失则不可用）

| 能力 | 竞品状态 | DeepX 现状 |
|------|---------|-----------|
| **内联代码补全** | Cursor/Copilot/Windsurf 标配 | ❌ 无编辑器集成 |
| **Diff 预览后确认应用** | 所有竞品 | ❌ 直接写盘，无 accept/reject |
| **多文件协同重构** | Cursor Agent / Claude Code | ⚠️ 逐文件操作，无依赖跟踪 |
| **聊天 + 代码库上下文** | Cursor Cmd+L / Copilot Chat | ⚠️ 有 explore 但缺光标/文件级记忆 |
| **自主规划执行** | 所有竞品已转向 Agent 模式 | ⚠️ 有 subagent 但缺多步规划 |
| **Git 集成 / PR 工作流** | Copilot Workspace 从 Issue→PR | ❌ 仅有 exec 跑 git 命令 |

### DeepX 可占据的蓝海

| 差异点 | 为何 DeepX 独有 |
|--------|----------------|
| **终端原生** | Cursor/Copilot/Windsurf 全绑定 IDE。SRE/DevOps/嵌入式开发者无好用的 AI 助手 |
| **Headless CI 模式** | 无竞品做 CI 管道内 AI 代理。`deepx --ci "fix all clippy warnings"` |
| **多 Session 并行** | 架构天然支持。竞品是单会话模型。可做 Side-by-Side、Spectate、Fork |
| **Subagent 生态** | 已有 spawn。可发展为 Agent Zoo、Bug Bounty |
| **MCP 插件系统** | `mcp_bridge.rs` 已有基础。可做 MCP Marketplace |

---

## 三、v0.5 功能路线

### P0 — 必须修复（阻塞产品质量）

| # | 功能 | 说明 | 复用 |
|---|------|------|------|
| 1 | **重构 Agent Bridge** | `AgentRegistry` 去掉全局 Mutex → `DashMap` + per-agent actor | agent_bridge 全部 |
| 2 | **多文件协同编辑** | 一次 agent 调用可规划并执行跨 N 个文件的修改 | explore + file_edit + task |
| 3 | **Diff 审批流程** | 工具产生 diff → 前端展示 → 用户 accept/reject → 落地 | edit_file / edit_file_diff |
| 4 | **Agent 任务规划器** | 从用户需求生成文件编辑计划 → 分步执行 → 自我验证 | task + subagent + explore |

### P1 — 差异化竞争力

| # | 功能 | 说明 | 复用 |
|---|------|------|------|
| 5 | **Session Tabs 并列** | 侧边栏多 tab，同时运行多个 agent，拖拽消息跨 session | chatStores Map 已有 |
| 6 | **Headless CI 模式** | `deepx --ci "task"` 无 Tauri 运行，输出结果码 | msglp 可独立运行 |
| 7 | **代码库语义索引** | `explore` 升级：符号级索引，支持"找到所有调用此函数的文件" | explore + rust-analyzer 思路 |
| 8 | **Git 工作流集成** | 原生 PR 创建、commit message 生成、branch diff 摘要 | exec + git |
| 9 | **模型智能路由** | 简单任务用便宜模型，复杂重构用贵模型 | config + gate |

### P2 — 独特体验（funny 分支方向）

| # | 功能 | 说明 | 复用 |
|---|------|------|------|
| 10 | **Agent Zoo** | 可视化所有运行中的 subagent，卡片式状态面板 | process_registry + StatusPanel |
| 11 | **Bug Bounty 模式** | "找出这个模块的 bug" → 并行 subagent 搜索 → 计分 | spawn_subagent + task |
| 12 | **Context Expose** | 交互式 treemap 显示 context window 组成 | SessionInfo + message store |
| 13 | **Code Heatmap** | 在 StockChart 上叠加文件级热力图 | code_stats.jsonl + lightweight-charts |
| 14 | **Session Fork** | 从任意 turn 分叉新 session，A/B 测试 AI 方案 | session 文件系统 |
| 15 | **Achievement 系统** | 勋章弹窗：千行代码/10 个 subagent/bug 猎手 | code_delta + activity log |

### P3 — 月球项目

| # | 功能 | 说明 |
|---|------|------|
| 16 | **Self-Healing Codebase** | 持续扫描技术债/安全漏洞/废弃 API，自动提 PR |
| 17 | **AI 根因调试** | 输入崩溃日志 → 追踪源码 → 定位根因 + 修复建议 |
| 18 | **NL → Full-Stack** | "帮我做一个 SaaS 计费系统" → schema + API + 前端 + 部署 |
| 19 | **Spectate Mode** | 实时观看另一个 session 的 agent 在工作 |
| 20 | **离线混合模型** | 本地小模型做补全，云端大模型做复杂任务 |

---

## 四、技术债务清理（v0.5 内完成）

| 项 | 说明 |
|----|------|
| **Seed newtype** | `SessionSeed(String)` 替代裸 String，含验证 |
| **存储 schema 版本** | `meta.json` 加 `schema_version: 1`，支持前向迁移 |
| **错误传播** | persistence 失败向上传播 Error 事件，而非静默吞 |
| **Config payload** | `cmd_save_config` 改为接收 JSON payload 而非 18 参数 |
| **handle_user_input 拆分** | 拆为 plan / execute / verify 三阶段 |
| **常量集中管理** | flush interval、backoff 统一到 config 或 const 模块 |

---

## 五、建议的 v0.5 执行顺序

```
Week 1-2:  P0.1 Agent Bridge 重构 (解锁后续所有并发改进)
           P0.4 Agent 任务规划器 (市场已转向 agent 模式)
           
Week 3-4:  P0.2 多文件协同编辑
           P0.3 Diff 审批流程
           
Week 5-6:  P1.5 Session Tabs (0.4.0-funny 已部分完成)
           技术债清理 (seed newtype, error propagation, config payload)
           
Week 7-8:  P1.6 Headless CI, P1.7 语义索引
           P2.10 Agent Zoo, P2.11 Bug Bounty
           
Week 9+:   P2.12-P2.15 体验功能
           文档 + 发布
```
