# DSX — 新架构计划：HP 合入 Agent

> 生成时间: 2026-05-23
> 基于完整源码阅读 + 可行性分析

---

## 一、核心决策

**HP 整体合入 dsx-agent**，不再作为独立进程运行。

| 理由 | 说明 |
|------|------|
| HP 唯一价值是 `anthropic_api.rs::chat_stream()` | ~340 行，调 DeepSeek API + 解析 SSE |
| 其余都是 IPC 模板代码 | TCP 监听、帧分发、进程注册/保活 ≈ 700 行无意义代码 |
| 安全边界不成立 | API key 本来就同时被 agent 和 HP 从同一配置文件读取 |
| 进程保活无必要 | 你说得对，agent 内直连即可 |

### 合并后进程拓扑

```
dsx-tauri (GUI)
  └── spawns dsx-agent (含 API 直连)
        └── spawns dsx-tools (子进程, 沙箱隔离)
```

**保留 dsx-tools 子进程**：工具崩溃不杀主进程，exec 沙箱安全隔离。

---

## 二、工作区结构现状（8 crates）

| Crate | 行数 | 合并后变化 |
|-------|------|-----------|
| dsx-types | 851 | 不变 |
| dsx-proto | 361 | 不变（tools 仍需） |
| dsx-agent | 6,168 | **+340**（搬入 anthropic_api） |
| dsx-hp | 1,257 | **整包删除** |
| dsx-tools | 2,885 | 不变 |
| dsx-sudo | 147 | 不变 |
| dsx-tauri | 631 | 不变 |
| dsx | 99 | **-3 行**（移除 hp 路由） |

---

## 三、具体改动

### 3.1 搬入文件

`dsx-hp/src/anthropic_api.rs` → `dsx-agent/src/anthropic_api.rs`

- 内容：`GatewayConfig`、`StreamEvent`、`chat_stream()`、`normalize_messages()`、`build_anthropic_url()`
- **不改一行逻辑**，纯搬运

### 3.2 dsx-agent/Cargo.toml

```toml
# 新增依赖
reqwest = { version = "0.13", default-features = false, features = ["rustls", "json", "stream"] }
futures-util = "0.3"
```

### 3.3 dsx-agent/src/lib.rs

```rust
// 新增一行
pub mod anthropic_api;
```

### 3.4 dsx-agent/src/runner.rs — 核心替换

**当前**（L228-303, L636-700）：两处 HP IPC 调用
1. 主 tool-calling loop：发送 `AgentToHp::ApiChat` → 循环读 `HpToAgent` 帧
2. Post-tool-loop：同上

**改为**：创建同步包装函数 `call_anthropic_api_sync()`：

```rust
fn call_anthropic_api_sync(
    config: &Config,
    model: &str,
    system: String,
    messages: Vec<Message>,
    effort: Option<String>,
    max_tokens: u32,
    tools: Option<Vec<ToolDef>>,
    session_seed: &str,
) -> Vec<ApiEvent> {
    // 1. 创建 tokio runtime（同 HP 做法）
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);

    // 2. 调用 anthropic_api::chat_stream
    let cfg = GatewayConfig { base_url: config.base_url.clone(), api_key: config.api_key.clone() };
    rt.block_on(async {
        let _ = anthropic_api::chat_stream(&cfg, model, Some(system), messages, tools, max_tokens, effort, Some(session_seed.into()), tx).await;
    });

    // 3. 收集事件
    let mut events = Vec::new();
    while let Some(event) = rt.block_on(rx.recv()) {
        events.push(event);
    }
    events
}
```

**替换后**（L228-303 变成）：

```rust
let events = call_anthropic_api_sync(
    &agent.config, &agent.config.model, system, msgs_no_system,
    agent.config.effort.clone(), agent.config.max_tokens,
    Some(agent.tool_defs.clone()), &agent.session_seed,
);

for event in events {
    match event {
        StreamEvent::ContentDelta(delta) => {
            // 写 tui 帧（同当前）
            let _ = dsx_proto::write_frame(tui_writer, &AgentToTui::ContentDelta { delta, reasoning: None });
        }
        StreamEvent::ReasoningDelta(delta) => {
            // 写 tui 帧
        }
        StreamEvent::ToolCallProgress { name, args_so_far } => {
            // 写 tui 帧
        }
        StreamEvent::Done { raw_message, usage, stop_reason } => {
            // 解析 content, tool_calls, reasoning_content
            // 继续当前流程（同当前）
            break;
        }
        StreamEvent::Error(msg) => {
            // 写 error 帧 + return
        }
        _ => {}
    }
}
```

**输出质量检测**（HP 的重复/退化检测）加在 `ContentDelta` 处理中。

### 3.5 删除 dsx-agent/src/hp.rs

整文件 83 行，不再需要 `connect()` 和 `try_reconnect()`。

### 3.6 dsx-agent/src/api.rs

当前 261 行，是 async orchestrator 路径的发帧代码。**可留作兼容层**或逐步替换。新 `anthropic_api::chat_stream` 直接可用时，此文件不再被调用。不影响当前 runner 主路径。

### 3.7 删除 dsx-hp/ 整个 crate

| 文件 | 行数 | 原因 |
|------|------|------|
| `dsx-hp/src/runner.rs` | 561 | TCP 服务器主循环 |
| `dsx-hp/src/anthropic_api.rs` | 340 | **已搬入 agent** |
| `dsx-hp/src/config.rs` | 7 | agent 已有 |
| `dsx-hp/src/types.rs` | 88 | 不再需要 |
| `dsx-hp/src/registry.rs` | 156 | 不再需要 |
| `dsx-hp/src/liveness.rs` | 76 | 不再需要 |
| `dsx-hp/src/ipc_traits.rs` | ~50 | 不再需要 |
| `dsx-hp/src/lib.rs` | 24 | 不再需要 |
| `dsx-hp/src/main.rs` | 6 | 不再需要 |
| `dsx-hp/Cargo.toml` | 15 | 不再需要 |

### 3.8 根 Cargo.toml

```toml
# 从 workspace members 中移除 "crates/dsx-hp"
```

### 3.9 dsx/src/main.rs

```rust
// 移除这行
"hp" => dsx_hp::runner::run(),
// 其他保持不变
```

---

## 四、改动汇总

| 操作 | 文件路径 | 行数变化 |
|------|---------|---------|
| **新建** | `crates/dsx-agent/src/anthropic_api.rs` | +340 |
| **改** | `crates/dsx-agent/Cargo.toml` | +3 |
| **改** | `crates/dsx-agent/src/lib.rs` | +1 |
| **改** | `crates/dsx-agent/src/runner.rs` | ~100 行替换 |
| **删** | `crates/dsx-agent/src/hp.rs` | -83 |
| **删** | `crates/dsx-hp/` (9 个文件) | -1,257 |
| **改** | `crates/dsx/src/main.rs` | -3 |
| **改** | 根 `Cargo.toml` | -1 |
| **总计** | **~12 个文件** | **净减 ~1000 行** |

---

## 五、风险与缓解

| 风险 | 缓解 |
|------|------|
| `tokio::runtime::Runtime` 每消息创建开销 | 用 `OnceLock` 缓存运行时（同 HP 现有做法） |
| `chat_stream` 阻塞 runner 主循环 | 当前 HP IPC 本来就是同步阻塞，无退化 |
| SSE 解析需 `reqwest` | 加依赖即可，HP 已有完整实现 |
| 输出质量检测丢失 | 搬入 agent 的 ContentDelta 处理逻辑中 |
| `dsx-proto` 中 `AgentToHp`/`HpToAgent` 部分变死代码 | 留着不动，不影响编译和运行 |

---

## 六、执行步骤（按顺序）

1. **新建** `dsx-agent/src/anthropic_api.rs` — 从 HP 原样复制
2. **改** `dsx-agent/Cargo.toml` — 加 `reqwest`, `futures-util`
3. **改** `dsx-agent/src/lib.rs` — 加 `pub mod anthropic_api`
4. **改** `dsx-agent/src/runner.rs` — 替换两处 HP IPC 为 `call_anthropic_api_sync`
5. **删除** `dsx-agent/src/hp.rs`
6. **删除** `crates/dsx-hp/` 整个目录
7. **改** 根 `Cargo.toml` workspace members
8. **改** `crates/dsx/src/main.rs`
9. **编译验证** `cargo build -p dsx-agent`
10. **测试** 流式消息、工具调用、取消操作、session 恢复

预计 **2 天**（1 天核心改动 + 1 天测试修复）。
