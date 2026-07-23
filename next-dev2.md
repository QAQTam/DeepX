# DeepX 下一阶段改进思路：Session Plan 隔离与 Goal 安全追加

> 状态：研究草案  
> 面向版本：v0.9.x → v1.0.0  
> 日期：2026-07-23  
> 用途：供后续模型、开发者继续做架构研究与实现方案评审

## 1. 问题背景

DeepX 已经从 Tauri 进程内后端逐步迁移到独立 `deepx-daemon`，并以 Electron 作为主要桌面前端。下一阶段需要解决 Plan 与 Goal 的数据边界问题，否则多 Session、多客户端以及未来 TUI 接入后，会出现难以解释的状态污染和并发写入风险。

本轮重点研究两个问题：

1. Goal 模式是否支持在执行中追加步骤，以及怎样追加才不会绕过用户授权。
2. 同一个工作区中，一个 Session 创建的 PLAN 是否会影响新 Session，以及怎样做到默认隔离、显式复用。

## 2. 当前实现结论

### 2.1 GoalRun 已经按 Session 隔离

当前 Goal 的运行态保存在：

```text
<data_dir>/sessions/<seed>/goal_run.json
<data_dir>/sessions/<seed>/goal_activation.json
```

因此不同 Session 不会直接共享同一个 `GoalRun`。新 Session 通常也不会自动显示旧 Session 的 Goal 进度条。

### 2.2 PLAN 仍然按 Workspace 共享

当前 Plan 默认保存在：

```text
<workspace>/.deepx/PLAN.md
```

没有绑定工作区时，则回退到：

```text
<data_dir>/workspace/PLAN.md
```

这意味着同一工作区中的多个 Session 会读写同一个 PLAN。新 Session 虽然没有旧 Session 的 GoalRun，但可以看到、提交或激活旧 Session 留下的 PLAN。

结论：当前存在真实的跨 Session Plan 污染；无工作区的 Session 还会共享同一个全局回退 PLAN，风险更大。

### 2.3 Goal 激活采用快照，但没有安全追加能力

Goal 激活时会把当前 PLAN 中符合条件的条目复制到 Session 自己的 `goal_run.json`。因此：

- 激活后再向 PLAN 添加条目，不会自动进入正在运行的 Goal。
- 当前没有正式的 Goal append API。
- 停止或完成后重新激活 PLAN，可能重新从头装载所有未拒绝条目，存在重复执行风险。
- 如果直接修改 `goal_run.json`，会绕过租约、generation、用户授权以及事件同步。

### 2.4 当前锁不能解决跨 Session 并发

Plan 写入使用的锁是进程内静态互斥锁。但当前 Agent worker 是每 Session 一个独立进程，因此该锁只能限制单个 worker 内并发，不能阻止两个 Session 同时修改同一份 Workspace PLAN。

潜在结果包括：

- 后写覆盖先写；
- Markdown 内容交叉或丢失；
- Desktop、TUI 和 Agent 看到不同版本；
- Goal 激活时读取到不一致快照。

## 3. 改进原则

### 3.1 默认隔离，显式共享

Session 自己创建的 Plan 默认只属于该 Session。Workspace 级 Plan 只能作为显式模板存在，必须由用户主动导入或复制。

### 3.2 Daemon 是唯一业务写入者

Desktop、TUI 和 Agent worker 都不应直接写 Plan/Goal 文件。所有修改经由 Daemon 的领域服务完成。

### 3.3 JSON 是规范数据，Markdown 是交换格式

Markdown 适合人阅读、Git diff 和导入导出，但不适合作为并发状态数据库。规范数据应使用结构化 JSON 或数据库记录，Markdown 只作为展示、导入和导出格式。

### 3.4 Goal 范围变化必须重新获得授权

用户批准的是一个明确的执行范围。Goal 执行中新增步骤属于扩大范围，必须展示差异并由持有 Session 租约的客户端确认。

### 3.5 所有写操作都可检测冲突

Plan 和 Goal 需要 `revision`；Goal 还需要 `generation`。请求携带期望版本，Daemon 拒绝陈旧写入，而不是默默覆盖。

## 4. 建议的目标数据模型

### 4.1 Session Plan

建议将规范 Plan 移到：

```text
<data_dir>/sessions/<seed>/plan.json
```

参考结构：

```json
{
  "schema_version": 1,
  "plan_id": "plan_...",
  "seed": 123,
  "workspace_id": "workspace_...",
  "revision": 7,
  "title": "实现流式事件管道",
  "status": "draft",
  "items": [],
  "created_at": "...",
  "updated_at": "..."
}
```

关键约束：

- `seed` 是所有权边界，而不仅是查询参数。
- 新 Session 不自动继承同工作区其他 Session 的 Plan。
- Plan 可以引用 workspace，但 workspace 不能反向决定 Plan 的唯一存储位置。
- 不再设置所有无工作区 Session 共用的全局 Plan。

### 4.2 GoalRun

建议升级为 schema v2：

```json
{
  "schema_version": 2,
  "goal_id": "goal_...",
  "plan_id": "plan_...",
  "seed": 123,
  "generation": 2,
  "revision": 12,
  "objective": "完成前后端事件管道升级",
  "status": "active",
  "items": [],
  "next_index": 3,
  "awaiting_next_turn": false,
  "paused_reason": null,
  "auto_turns": 4,
  "approved_scope_hash": "sha256:...",
  "created_at": "...",
  "updated_at": "..."
}
```

字段职责：

- `generation`：区分同一 Session 中不同轮 Goal 激活。
- `revision`：检测并发修改和陈旧响应。
- `approved_scope_hash`：记录用户最后批准的执行范围。
- `next_index`：避免重新激活时从头执行。
- `auto_turns`：给自动推进设置可观察、可限制的预算。

### 4.3 Workspace Plan Template

Workspace 级数据不再是活跃 Session 的共享 Plan，而是模板：

```text
<workspace>/.deepx/plan-templates/<template_id>.json
```

支持以下显式操作：

- 从 Workspace 模板导入到当前 Session；
- 将当前 Session Plan 保存为 Workspace 模板；
- 导出为 Markdown；
- 从 Markdown 导入为新的 Session Plan。

导入应采用复制语义，生成新的 `plan_id`，而不是让多个 Session 继续引用同一可变对象。

## 5. Goal 安全追加设计

Goal 可以支持追加，但不建议允许 Agent 静默追加并自动执行。推荐两阶段协议：

```text
Agent 提出追加建议
  → Daemon 创建 GoalAppendProposal
  → Desktop/TUI 展示步骤差异与原因
  → 用户批准或拒绝
  → Daemon 校验租约、generation、revision
  → 原子写入 GoalRun
  → 发布新 Snapshot/Event
```

### 5.1 Proposal 建议字段

```json
{
  "proposal_id": "proposal_...",
  "seed": 123,
  "goal_id": "goal_...",
  "generation": 2,
  "expected_revision": 12,
  "reason": "发现必须先迁移旧事件格式",
  "items": [],
  "created_at": "...",
  "expires_at": "..."
}
```

### 5.2 追加约束

- 仅 `active` 或 `paused` Goal 可追加。
- 已完成 Goal 不得静默复活；需要用户显式创建新 Goal 或新 generation。
- 只允许追加到队尾，不能修改当前步骤或已完成步骤。
- item ID 必须唯一。
- 依赖必须存在，且依赖图不能成环。
- 每次及总步骤数应有上限。
- Proposal 只能响应一次，并设置过期时间。
- 只有持有当前 Session 租约的客户端可以批准。
- 响应时同时校验 `goal_id`、`generation`、`expected_revision`。
- 批准后重新计算 `approved_scope_hash`，写入审计记录。

### 5.3 是否要求先暂停

建议第一版要求 Goal 进入 `paused` 或 `awaiting_approval` 后再批准追加。这样更容易保证当前执行步骤和追加事务之间没有竞态。后续如果证明 actor 串行化足够可靠，再考虑 active 状态下无停顿追加。

## 6. Daemon 与 Agent 的职责重划

### 6.1 新增领域服务

建议在 `deepx-runtime` 中建立：

```text
PlanService
GoalService
WorkspaceTemplateService
```

它们负责：

- 权限与 Session 租约校验；
- revision/generation 校验；
- 规范化和业务规则验证；
- 原子持久化；
- Snapshot 投影；
- 事件发布；
- request_id 幂等处理。

### 6.2 每 Session 串行化

Daemon 应为每个 Session 使用 actor、队列或独立 mutex，使同一 Session 的所有 Plan/Goal 命令按序执行。不同 Session 可以并行。

文件落盘至少应采用“写临时文件 + flush + 原子替换”。如果 Turso 已经稳定承担会话状态，也可以研究将 Plan/Goal 写入事务数据库，并保留 JSON 镜像用于调试和兼容。

### 6.3 Agent 不再直接写业务文件

Agent worker 只产生结构化意图，例如：

```text
PlanMutationRequested
GoalStepCompleted
GoalAppendProposed
GoalPauseRequested
```

Daemon 验证后决定是否修改状态。需要进一步研究 worker → daemon 内部通道的实现：

1. 扩展现有 Agent2Ui，使其能表达领域 mutation request；
2. 建立双向内部 RPC；
3. 短期使用跨进程锁和原子文件替换作为过渡。

推荐 1 或 2。方案 3 只能作为临时补丁，因为它仍把业务规则分散在 worker 内。

## 7. 控制协议草案

建议增加或整理以下方法：

```text
plan.get
plan.create
plan.update
plan.delete
plan.submit
plan.import_workspace_template
plan.export_workspace_template

goal.get
goal.activate
goal.pause
goal.resume
goal.stop
goal.append.propose
goal.append.respond
goal.step.complete
```

所有写请求至少包含：

```text
request_id
seed
expected_revision
plan_id 或 goal_id
generation（Goal 请求）
```

建议稳定错误码：

```text
plan_not_found
goal_not_found
goal_not_active
stale_revision
stale_generation
goal_scope_approval_required
goal_append_conflict
goal_step_limit
invalid_dependency
session_lease_required
proposal_already_resolved
```

Snapshot 应包含当前 Plan、Goal、未处理 Proposal 以及 revision/generation，使 Electron 与未来 TUI 不需要自行拼接状态。

## 8. Desktop UI 改进

### 8.1 Session 切换

切换 Session 时，前端必须以 Snapshot 中的 `seed + goal_id + generation` 作为组件身份。不能保留上一 Session 的 Goal 本地状态。

### 8.2 Goal 完成后的收起

Goal 状态条应由后端 canonical 状态驱动：

- `active` / `paused` / `awaiting_approval`：显示；
- `completed`：短暂显示完成反馈后自动收起；
- `stopped` / `failed`：按产品设计保留摘要或收起；
- 收到新 generation 时重建本地展示状态。

### 8.3 Append 审批界面

界面至少展示：

- 为什么需要追加；
- 新增哪些步骤；
- 对完成时间和权限范围的影响；
- 当前 revision 是否已过期；
- 批准、拒绝、编辑后批准三个动作。

编辑后批准应创建新的 Proposal 或新的 revision，不能直接修改原 Proposal 后复用旧签名。

## 9. 旧数据迁移

迁移原则：不自动删除或覆盖现有 `.deepx/PLAN.md`。

当 Daemon 检测到旧 PLAN 时，提供一次性选择：

1. 导入到当前 Session；
2. 保存为 Workspace 模板；
3. 暂时跳过。

迁移要求：

- 导入成功前不改动旧文件；
- 解析失败时不产生半成品；
- 只有用户确认后才重命名或归档旧文件；
- 记录来源路径、内容 hash 和导入时间，避免重复导入；
- Goal v1 → v2 原地升级时保留当前索引、已完成步骤和暂停状态；
- 无法可靠归属到某个 Session 的旧 PLAN，不应自动猜测归属。

## 10. 分阶段实施建议

### Phase A：先消除跨 Session 污染

- 引入 Session `plan.json`。
- Plan API 强制携带 seed。
- 新 Session 不再自动读取 Workspace PLAN。
- Workspace PLAN 只通过显式导入兼容。
- 添加两个 Session 同工作区的隔离测试。

### Phase B：收归 Daemon 写权限

- 实现 PlanService / GoalService。
- Agent worker 停止直接写 Plan/Goal 文件。
- 加入 revision、generation 和 request_id 幂等。
- 同 Session 命令串行化。

### Phase C：实现 Goal Append

- 新增 Proposal 模型与协议。
- 实现审批、过期、冲突和审计。
- Electron 增加差异确认 UI。
- 为 TUI 保持协议和 `deepx-client` API 可复用。

### Phase D：删除旧共享实现

- 移除全局 fallback PLAN。
- 移除进程内 `PLAN_LOCK` 作为一致性保障的职责。
- 移除 worker 对 Workspace PLAN 的直接写入。
- 完成旧 PLAN 的导入和归档工具。

## 11. 测试重点

### 11.1 隔离与并发

- 同工作区两个 Session 创建 Plan，彼此不可见。
- 无工作区的两个 Session 不共享 Plan。
- 两个客户端同时修改同一 Session，只有租约持有者成功。
- 同 revision 的两个写请求，后到者返回 `stale_revision`。
- Daemon 重启后 Snapshot、revision 和 Goal 进度一致。

### 11.2 Goal Append

- active/paused Goal 能发起 Proposal。
- 非租约持有者不能批准。
- generation 或 revision 过期时拒绝。
- 相同 request_id 重试不会重复追加。
- 不允许修改当前或已完成步骤。
- 不允许循环依赖和超出步骤上限。
- 已完成 Goal 不会因为追加请求自动复活。
- Daemon 重启后未过期 Proposal 可以恢复，或按明确规则失效。

### 11.3 迁移

- 旧 PLAN.md 能导入为 Session Plan。
- 导入不会改变原文件。
- 相同旧文件不会重复导入。
- 格式损坏时不创建 Plan。
- Goal v1 升级后继续正确的 next_index。

### 11.4 UI

- Session 切换不残留 Goal 状态。
- completed 后进度条自动收回。
- Append Proposal 能显示差异、批准和拒绝。
- stale Snapshot 不覆盖更新的 revision。
- 断线重连后由 Snapshot 恢复，而不是依赖前端缓存猜测。

## 12. 需要后续模型重点研究的问题

1. Plan/Goal 的规范存储应继续使用 JSON 文件，还是迁移到 Turso，并把 JSON 作为镜像？
2. Agent worker 向 Daemon 提交 mutation request，应该扩展现有内部事件协议还是建立独立双向 RPC？
3. Goal Append 能否复用 Ask/Plan 的交互信封，还是需要独立 Proposal 生命周期？
4. `approved_scope_hash` 应覆盖哪些字段，怎样避免仅调整文案就导致无意义失效？
5. Append 是否必须先暂停 Goal，还是每 Session actor 串行化已足够？
6. 自动 Goal turn 的次数、时间和工具权限应该怎样限制？
7. 是否仍允许用户直接编辑 Markdown；若允许，如何转成带 revision 的结构化更新？
8. Workspace Template 是否需要纳入 Git，默认是否应该被 `.gitignore`？
9. 未来是否需要高级模式：多个 Session 共同引用一个只读 Plan 模板并各自执行？
10. 旧 Workspace PLAN 无法识别创建 Session 时，产品应如何提示归属？

## 13. 推荐决策顺序

建议不要先从 Goal Append 开始。正确顺序是：

```text
Session Plan 默认隔离
  → Daemon 成为唯一写入者
  → revision / generation / 幂等
  → Workspace Plan 改为显式模板
  → 用户批准的 Goal Append
```

如果隔离和写入权威尚未解决，直接增加 Goal Append 会把现有共享文件和跨进程竞态进一步放大。

## 14. 预期最终效果

完成后，DeepX 的 Plan/Goal 模型应具备以下性质：

- 每个 Session 有独立计划和执行状态；
- 工作区只提供可显式复用的模板，不会污染新对话；
- Electron、未来 TUI 和其他客户端观察同一个 Daemon 权威状态；
- Agent 可以提出扩展目标，但不能绕过用户批准；
- 断线、重连、Daemon 重启和请求重试不会重复执行或丢失步骤；
- v1.0.0 前锁定清晰的数据所有权、并发语义和跨前端协议。
