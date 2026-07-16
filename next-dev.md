# DeepX Agent Runtime 微内核与插件化重构交接报告

## 1. 交接目标

下一阶段目标不是继续向 `deepx-msglp::Loop` 增加 Engine，而是将其收缩为一个只负责事件调度、effect 执行、取消和停机的微内核。业务插件拥有各自的局部状态机，通过稳定的 typed contract 自行推进工作流，但不能直接控制全局事件循环或任意修改 `AgentState`。

推荐首先实现“编译期插件注册 + InteractionPlugin”，暂不直接引入 WASM、原生动态库或完整多 session actor runtime。

## 2. 当前工作区与前置成果

- 工作树：`F:\DeepX-Fork-worktrees\skills-context-manager`
- 分支：`codex/skills-context-manager`
- 当前 Skills 全链路改造尚未提交；后续窗口必须在上述工作树继续，不要从 `F:\DeepX-Fork` 主工作区重新实现。
- 已完成独立 `SkillContextManager`、V2 session state、权威尾部 envelope、有序 `Vec<ToolEffect>`、skills V2 IPC 和前端五栏工作台。
- Skills 是后续插件化的参考实现：状态不再由 `MessageStore` 或历史 system message 推断，正文、租约、revision、恢复和 UI operation 均由单一管理器负责。

当前验证证据：

- `cargo check --workspace`：通过。
- 相关 Rust 定向测试：114 项通过。
- 前端定向 Vitest：29 项通过。
- `npm run build`：通过。
- `git diff --check`：通过。
- 已知基线：`deepx-gate::tool_parser::tests::test_has_dsml_detection` 仍失败；`deepx-msglp` 的 `concurrent_read_stress` 存在既有挂起问题，不得误报为本轮回归。

## 3. 当前 `deepx-msglp` 的主要问题

现有 `Engine` trait 只是半插件化接口：

- `new/loop_core.rs` 约 1156 行，仍直接处理 session、skills、compact、undo、permission、ask、plan 和 UI event。
- `new/engine_turn.rs` 约 1158 行，同时承担 provider 调用、工具 round、并行调度、暂停、恢复和 turn 终止。
- `new/engine_tool.rs` 约 608 行，授权、执行、progress、pending state 和 batch admission 仍耦合。
- `TurnEngine`、`CompactEngine`、`MiscEngine` 等没有真正通过统一 registry 调度，Loop 中仍有大量显式 match 和直接调用。
- `RingContext` 向 Engine 暴露完整 `&mut AgentState`，无法形成 capability boundary。
- `Loop::apply_outcome` 知道 Skills 生命周期、session flush、通知和具体 IPC 事件，业务语义仍泄漏进宿主。

结论：下一阶段应建立 `KernelEvent -> PluginDecision -> KernelEffect -> KernelEvent` 闭环，而不是继续扩充 `Outcome` 和 fallback match。

## 4. 目标架构

### 4.1 Loop 微内核仅保留五项职责

1. 从 Transport/mailbox 接收 `KernelEvent`。
2. 根据 `PluginRegistry` 将事件确定性投递给插件。
3. 收集、校验并按稳定顺序提交 `KernelEffect`。
4. 维护全局 phase、取消、shutdown 和 effect journal。
5. 将 effect 执行结果转回新事件，直到进入 Idle、Suspended 或 Shutdown。

Loop 不应了解 skill、permission、ask_user、plan、provider、compact 或 session 的具体业务语义。

### 4.2 插件拥有局部 workflow，不拥有全局 loop

例如 Turn 插件内部状态机可以是：

```text
TurnRequested
  -> ProviderRequested
  -> ModelResponseReceived
  -> ToolBatchRequested
  -> ToolBatchCompleted
  -> ProviderRequested
  -> TurnCompleted
```

插件每次只返回下一批 effect。内核执行 effect 后把结果作为新事件重新投递。禁止插件自行递归、阻塞主循环或维护另一份全局 phase。

### 4.3 建议公共契约

DTO/trait 必须先落地，消费者不得依赖另一插件的内部结构。

```rust
pub trait LoopPlugin: Send {
    fn descriptor(&self) -> PluginDescriptor;

    fn on_event(
        &mut self,
        event: &KernelEvent,
        ctx: &PluginContext<'_>,
    ) -> PluginDecision;

    fn snapshot(&self) -> Option<PluginSnapshot>;
    fn restore(&mut self, snapshot: PluginSnapshot) -> Result<(), PluginError>;
}

pub enum PluginDecision {
    Pass,
    Effects(Vec<KernelEffect>),
    Suspend(Suspension),
    Reject(PluginError),
}
```

首批公共类型建议包括：

- `PluginId`
- `PluginDescriptor`
- `PluginCapability`
- `KernelEvent`
- `KernelEffect`
- `PluginDecision`
- `Suspension` / `SuspensionKey`
- `PluginSnapshot { plugin_id, schema_version, revision, payload }`
- `PluginError { code, plugin_id, operation_id, retryable, message }`

这些类型应保持纯 DTO，不引用具体 Engine 或 Manager 内部类型。

### 4.4 缩窄 PluginContext

插件不能获得 `&mut AgentState`。建议通过能力端口访问宿主：

```text
PluginContext
|- MessagePort
|- ProviderPort
|- ToolPort
|- SessionPort
|- EventPort
|- PolicyPort
|- Clock
`- CancelToken
```

每个插件只得到 descriptor 声明过的 capability。所有持久化、IPC、文件或工具副作用都必须变成 `KernelEffect`，由宿主统一授权和执行。

## 5. 后续可拆分组件

### P0：InteractionManager / InteractionPlugin

统一管理 permission、ask_user 和 plan review：

- suspension 创建、排队和恢复；
- request identity、session identity、turn identity；
- stale/duplicate response；
- cancel、undo、session switch 失效；
- 多 permission、多 ask 和 plan 的确定性顺序；
- snapshot 仅保存稳定状态，进程重启时明确终止不可恢复的交互。

这是最优先拆分项，因为三类交互共享同一种暂停/恢复语义，也是 TurnEngine 当前最高风险的耦合点。

### P1：ToolRoundCoordinator

负责：

- tool batch admission；
- authorization challenge；
- 并行执行和同资源冲突排序；
- progress multiplex；
- 原始 tool-call 顺序提交 `Vec<ToolEffect>`；
- tool round 完成或暂停事件。

Turn 插件只能请求 `ExecuteToolRound`，不能了解线程、permission queue 或具体工具执行器。

### P1：ProviderConversationManager

负责：

- stateless/stateful provider 会话；
- remote session id 和重建；
- authoritative snapshot 同步；
- SSE/stream 生命周期；
- provider capability 与 tail-system 支持；
- cache/fallback 诊断。

### P1：ContextPipeline

负责确定性上下文装配：

```text
base system
stable catalog slots
conversation history
user annotations
tool results
authoritative tail envelopes
```

Skills、Policy 等插件通过命名 slot 提交 context fragment，不允许直接修改 `MessageStore`。

### P2：SessionRuntimeManager

负责 create/resume/switch/save，并保存：

```text
HashMap<PluginId, PluginSnapshot>
```

插件自行负责 schema version 与 migration；Session 层不解析插件业务 payload。

### P2：PolicyManager

统一 PLAN/CODE、compliance、permission level、capability grant 和执行策略。工具和插件只消费策略结果，不直接读取全局 config 决策。

### P2：ProjectionPlugin 与 SubagentSupervisor

- ProjectionPlugin：将内核事件投影为 dashboard、activity、toast 和 IPC 快照。
- SubagentSupervisor：管理 spawn、取消、资源预算、父子 turn 与进程回收。

## 6. 推荐实施批次

### Batch 0：冻结契约和回归锚点

- 定义纯 DTO 和 trait，不改运行路径。
- 为当前 command/outcome 建立 contract tests。
- 固化 ask_user、permission、skills、cancel、undo、session switch 回归。
- 禁止新增直接访问 Engine 内部字段的跨模块代码。

### Batch 1：InteractionPlugin

- 引入统一 `SuspensionBroker`。
- 把 pending permission、ask、plan 状态从 Turn/Tool 中迁出。
- 保持现有 IPC 不变，通过 adapter 转成 KernelEvent。
- 完成后 TurnEngine 不再拥有三类交互队列。

### Batch 2：ToolRoundPlugin

- 迁移 admission、并行执行、冲突序列化和 ordered effects。
- InteractionPlugin 通过 event/effect 与 ToolRoundPlugin 协作，不互相引用内部类型。

### Batch 3：Context 与 Provider

- 抽离 ContextPipeline 和 ProviderConversationManager。
- TurnWorkflowPlugin 只保留回合状态转换。
- Skills 改为正式 context-fragment plugin，但保留现有 `SkillContextManager` 作为插件内部实现。

### Batch 4：Registry 驱动 Loop

- 用 descriptor/registry 替换 `try_handle_via_engines` 和 fallback match。
- 将 `apply_outcome` 改为通用 effect executor。
- 将 `RingContext` 替换为 capability-scoped `PluginContext`。
- 目标：`loop_core.rs` 控制在约 300 行以内。

### Batch 5：可选动态装载

只有在编译期 contract 稳定后，才评估：

- actor-per-plugin；
- WASM adapter；
- 第三方插件签名、资源配额和沙箱。

不要直接从当前 Engine trait 跳到动态库 ABI。

## 7. 不可破坏的全局不变量

- 同一时刻一个 session 最多一个 running/suspended user turn。
- 任何异步响应必须同时绑定 session、turn、operation/request identity。
- Cancel/Abort 只能产生一个 terminal transaction。
- ToolEffect 必须按模型原始 tool-call 顺序提交。
- UI 晚到事件必须按 seed/revision 丢弃。
- 插件不能直接发送未登记 IPC 事件或绕过 Policy/Permission。
- 插件不能通过读取历史消息恢复权威状态。
- session switch、undo、shutdown 必须跨插件执行原子 invalidation。
- effect 执行失败必须返回结构化结果，禁止静默降级。

## 8. 首个窗口任务

下一窗口只负责 Batch 0 和 InteractionPlugin 设计，不同时修改 ToolRound、Provider 或前端视觉层。

建议先完成：

1. 调查 `loop_core.rs`、`engine_turn.rs`、`engine_tool.rs` 中所有 suspension 路径。
2. 输出 permission、ask_user、plan review 的统一状态转换表。
3. 定义 `KernelEvent`、`KernelEffect`、`Suspension`、`PluginDecision` 的最小 DTO。
4. 明确哪些类型进入 `deepx-types` / `deepx-proto`，哪些保持 `deepx-msglp` 内部。
5. 提交设计和 migration map，等待确认后再写实现计划。

不要立即：

- 新建 WASM runtime；
- 删除现有 Engine；
- 大范围移动 crate；
- 修改 Skills 已验证的状态机；
- 同时重写 IPC 和前端交互；
- 把 `AgentState` 原样包装进新 PluginContext。

## 9. 验收标准

- 新增业务插件无需修改 `Loop::dispatch` 或新增 fallback match。
- InteractionPlugin 可独立表驱动测试 permission/ask/plan 全生命周期。
- TurnWorkflow 不再保存交互队列或直接处理 UI response。
- PluginContext 不暴露完整 `AgentState`。
- 每个插件拥有版本化 snapshot，Session 只做 opaque persistence。
- Cancel、undo、session switch 的跨插件 invalidation 有集成测试。
- Skills、ask_user、permission 和现有 session restore 回归继续通过。
- `cargo check --workspace` 与相关定向测试通过；已知基线失败必须单独记录。

## 10. 给下一窗口的直接指令

请在 `F:\DeepX-Fork-worktrees\skills-context-manager`、分支 `codex/skills-context-manager` 上工作。先阅读本文件、`crates/deepx-skills/DESIGN.md`、`crates/deepx-msglp/src/new/types.rs`、`loop_core.rs`、`engine_turn.rs`、`engine_tool.rs` 和 `tests/ask_user_lifecycle.rs`。

本轮只进行 Agent Runtime 微内核化的 Batch 0 与 InteractionPlugin 设计：先定义 DTO/trait 和迁移边界，禁止直接依赖其它组件内部实现。先报告当前 suspension call paths、状态表、候选 contract 和风险，再等待批准进入代码修改。
