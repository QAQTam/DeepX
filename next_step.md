# DSX — 全面评估报告 & 下一步计划

> 生成时间: 2026-05-23
> 评估方式: 多智能体并行分析（架构 / 后端 / 前端）

---

## 一、现状总览

### 1.1 工作区结构（8 crates，~12,287 行 Rust）

| Crate | 行数 | 角色 |
|---|---|---|
| **dsx-types** | 851 | 共享类型、序列化、平台路径、token 计数 |
| **dsx-proto** | 307 | IPC JSON-LP 帧协议定义 |
| **dsx** | 89 | CLI 伞状入口（分派到 agent/hp/tools） |
| **dsx-agent** | 6,168 | **核心 AI 引擎**：编排器、会话、路由、技能、健康、HP 桥接 |
| **dsx-hp** | 1,257 | HP 守护进程（API 代理网关、进程注册、健康监控） |
| **dsx-tools** | 2,885 | 工具执行沙箱（文件、exec、web、计划、任务） |
| **dsx-sudo** | 147 | setuid 提权助手（零依赖） |
| **dsx-tauri** | 583 | Tauri GUI 后端（20 个命令、13 个事件） |

### 1.2 前端（~1,717 行 TS/TSX）

- **框架:** Preact 10 + Signals 2（通过 compat 层运行 React 库）
- **状态:** Zustand 5（持久状态）+ Signals 2（高频 stream）+ React Query 5（服务端缓存）
- **组件:** 8 个导出组件 + 12 个内部辅助组件
- **构建:** Vite 8 + Tailwind CSS 4 + @preact/preset-vite

### 1.3 运行时进程拓扑

```
┌──────────┐   stdin/stdout   ┌────────────┐   stdin/stdout   ┌───────────┐
│ dsx-tauri │ ←──JSON-LP────→ │ dsx-agent  │ ←──JSON-LP────→ │ dsx-tools │
│ (GUI)     │                 │ (编排器)    │                  │ (工具执行) │
└──────────┘                 └──────┬─────┘                  └───────────┘
                                    │ TCP localhost
                               ┌────┴─────┐
                               │  dsx-hp  │ ←──→ DeepSeek API
                               │ (网关)    │
                               └──────────┘
```

---

## 二、关键问题评估

### 2.1 架构问题：子进程模式

dsx-tauri 通过 `std::process::Command` 启动 dsx-agent 子进程，通过 stdin/stdout 管道交换 JSON 帧。

**弊端：**

| 问题 | 影响 |
|------|------|
| JSON 序列化/反序列化每一帧 | 不必要的 CPU 开销 |
| `spawn_agent` 竞态 | agent 未就绪时 `send_message` 可能失败 |
| `std::thread::sleep(500ms)` 等进程退出 | 500ms 延迟 + 不可靠 |
| 进程管理代码 ~120 行 | `find_dsx`、`spawn_agent`、`restart_agent`、`stop_agent`、`reload_agent` 五个函数 |
| 错误需拼接字符串解析（如 402 状态码） | 脆弱，已在 `agent-error` 事件中遇到 |
| `build.rs` 复制 dsx.exe + `beforeDevCommand` 构建 | 构建流程复杂 |

**Merge 后可直接消除所有上述问题。** dsx-agent 已经是 `pub fn runner::run()` 的库，Tauri 后端直接 `mpsc::channel` 驱动即可。

### 2.2 前端 Bug

| Bug | 状态 | 根因 |
|-----|------|------|
| react-markdown `createFile` assertion | **未修复** | preact/compat 将 children 传递为 VNode 对象而非 string |
| DOM 双棵树 | **未修复** | 可能由 react-markdown 崩溃引发渲染中断 → `_prevVNode` 被破坏 |
| newSession 后 UI 移位 | **未修复** | sessionId 从 '' 到值的跳变 + 双 DOM 树叠加 |
| Streaming badge 闪烁 | **未修复** | streamMode 从 idle→think→answer 跳变 |

### 2.3 已修复的问题

- `--muted` 加深（#9aa0a6 → #6b7280），通过 WCAG AA
- 所有 `text-[10px]` 提升到 11-12px
- 图标按钮字号加大（text-lg / text-base）
- 余额梯度颜色（负=红，正=绿，零=灰）
- body + 三面板添加 `contain: layout paint style`，缓解 resize 卡顿

---

## 三、Merge 方案：dsx-agent → dsx-tauri

### 3.1 目标架构

```
┌──────────────────────────────────────────┐
│  dsx-tauri (Tauri v2)                    │
│  ├── Rust 后端                            │
│  │   ├── Tauri commands (~20)             │
│  │   ├── mpsc::channel → agent 事件循环    │
│  │   └── 直接调用 dsx-agent 函数           │
│  ├── Preact 前端（不变）                    │
│  └── dsx-hp（子进程启动）                   │
│                                           │
│  移除: find_dsx / spawn_agent /            │
│        restart_agent / stop_agent /        │
│        reload_agent / build.rs /           │
│        beforeDevCommand cargo build        │
└──────────────────────────────────────────┘
```

### 3.2 具体改动

#### A. Cargo.toml 依赖

```toml
# dsx-tauri/src-tauri/Cargo.toml 新增
dsx-agent = { path = "../../dsx-agent" }
# 移除: 不再需要 dsx-types（agent 已传递）
```

#### B. AgentState 改造

```rust
// 当前
struct AgentState {
    stdin: Mutex<Option<Box<dyn Write + Send>>>,
}

// 改为
struct AgentState {
    tx: Mutex<Option<tokio::sync::mpsc::UnboundedSender<AgentCommand>>>,
    handle: Mutex<Option<std::thread::JoinHandle<()>>>,
}

enum AgentCommand {
    UserInput(String),
    ToolConfirm { id: String, approved: bool },
    Cancel,
    SetPhase(String),
    Shutdown,
}
```

#### C. 事件监听改造

当前：reader 线程从 stdout 读取 JSON → `app.emit("xxx", event)`
改为：agent 事件循环直接调用 `app.emit()` 或通过 channel 回传

```rust
// agent 线程内部：
let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
std::thread::spawn(move || {
    let mut agent = AgentState::new(config);
    // 不再需要 stdin/stdout BufReader
    // 改为从 rx 接收命令
    loop {
        tokio::select! {
            cmd = rx.recv() => {
                match cmd {
                    AgentCommand::UserInput(text) => {
                        // 直接调用 orchestrator::handle_send_message
                        handle_user_input(&mut agent, &text, &app)?;
                    }
                    AgentCommand::Shutdown => break,
                    ...
                }
            }
            // 事件直接通过 app.emit 发送
        }
    }
});
```

#### D. 可删除的文件/代码

| 文件/代码 | 行数 | 原因 |
|-----------|------|------|
| `dsx-tauri/src-tauri/build.rs` | 16 | 不再需要复制 dsx.exe |
| `dsx-tauri/src-tauri/src/lib.rs` 中 `find_dsx()` | 37 | 不再寻找二进制 |
| `dsx-tauri/src-tauri/src/lib.rs` 中 `ensure_hp()` | 40 | 保留，但改用 crate API |
| `dsx-tauri/src-tauri/src/lib.rs` 中 `spawn_agent()` | 28 | 不再需要 |
| `dsx-tauri/src-tauri/src/lib.rs` 中 `start_reader()` | 45 | 不再需要 |
| `dsx-tauri/src-tauri/src/lib.rs` 中 `start_stderr_reader()` | 24 | 不再需要 |
| `dsx-tauri/src-tauri/src/lib.rs` 中 `restart_agent()` | 28 | 不再需要 |
| `dsx-tauri/src-tauri/src/lib.rs` 中 `reload_agent()` | 18 | 改为直接调用 |
| `dsx-tauri/src-tauri/src/lib.rs` 中 `stop_agent()` | 4 | 不再需要 |
| `dsx` crate 整体（`crates/dsx/`） | 89 | 伞状入口不再需要 |
| `tauri.conf.json` 中 `beforeDevCommand` | — | 移除 `cargo build -p dsx` |
| `tauri.conf.json` 中 `beforeBuildCommand` | — | 同上 |
| `bundle.resources` 配置 | — | 不再捆绑 dsx.exe |
| 根 `package.json` 中 tauri 脚本 | — | 简化 |

#### E. 需要保留的进程

- **dsx-hp**: 仍作为独立子进程启动。理由：
  - 安全边界：持有 API Key
  - 独立生命周期：可在 GUI 关闭后继续运行
  - 复用现有 TCP/IPC 协议

- **dsx-tools**: 仍作为独立子进程启动。理由：
  - 沙箱隔离：工具执行崩溃不杀死主进程
  - 权限分离：可用不同用户运行

### 3.3 风险与注意事项

| 风险 | 缓解 |
|------|------|
| agent 事件循环中有阻塞操作（HP API 调用、工具 IPC） | 使用 `tokio::task::spawn_blocking` 或独立线程池 |
| `tokio` 运行时冲突（Tauri 可能已有自己的） | 确保使用 `tokio::runtime::Runtime::new()` 或 Tauri 的 async 运行时 |
| agent 库中 `println!`/`eprintln!` 用于 TUI 输出 | 需要改为通过回调/事件发出 |
| 现有 `dsx-proto` IPC 帧类型 `TuiToAgent`/`AgentToTui` 中的 JSON 序列化 | 可以逐步替换为直接函数调用，或保留帧作为内部命令枚举 |
| session 持久化逻辑耦合在 agent 中 | 保持现有代码不变，只是不再通过帧触发 |

---

## 四、前端 Bug 修复方案

### 4.1 react-markdown `createFile` assertion

**根因:** react-markdown 内部 `createFile(options)` 判断 `typeof children !== 'string'` 时抛错。preact/compat 将 JSX 的 string children 包装为 VNode。

**修复方案：** 确保传给 ReactMarkdown 的 children 在 JSX 编译后仍为原始 string。可能需要在调用处显式用 `String()` 包裹。

### 4.2 DOM 双棵树

**根因:** 怀疑是 react-markdown 崩溃时 Preact 的 `_prevVNode` 被破坏，导致下次 render 创建新树而不是 diff 现有树。

**修复方案：**
1. 先修复 react-markdown assertion（上述 4.1）
2. 在 `main.tsx` 的 `render()` 调用前加 `document.getElementById('root')!.innerHTML = ''` 确保干净挂载点
3. 如果仍出现，检查是否有 HMR 导致模块二次执行

### 4.3 newSession 后 UI 移位

**根因：** 双 DOM 树导致两套 app 实例堆叠

**修复：** 解决 4.2 后，自然消失

---

## 五、下一步执行计划（按优先级）

### Phase 1: Bug 修复（1-2 天）

- [ ] 修复 react-markdown + preact/compat children assertion（4.1）
- [ ] 修复 DOM 双棵树（4.2）
- [ ] 验证 newSession UI 移位已解决

### Phase 2: Merge dsx-agent → dsx-tauri（2-3 天）

- [ ] `dsx-tauri/Cargo.toml` 添加 `dsx-agent` 依赖
- [ ] 改造 `AgentState`：`Box<dyn Write>` → `mpsc::UnboundedSender<AgentCommand>`
- [ ] 创建 agent worker 线程，直接调用 `dsx_agent::orchestrator` 函数
- [ ] 事件直接从 worker 线程 `app.emit()`
- [ ] 删除子进程管理代码：`find_dsx`、`spawn_agent`、`start_reader`、`start_stderr_reader`、`restart_agent`、`stop_agent`、`reload_agent`
- [ ] 删除 `build.rs`、更新 `tauri.conf.json`
- [ ] 简化根 package.json、移除 `crates/dsx`（可暂留空壳）
- [ ] 测试：stream 消息、工具调用、session 恢复、取消操作

### Phase 3: 代码清理 & 可选改进（1-2 天）

- [ ] 移除不再使用的 `dsx-proto` 依赖（agent 内部帧不再需要）
- [ ] 将前端 `e: any` 类型替换为正确的事件类型定义
- [ ] 统一错误处理：`Result<_, String>` → thiserror 枚举
- [ ] 小范围微交互（按钮 hover/active 动画、新消息入场动画）
- [ ] 将 `beforeDevCommand` 中 `cargo build -p dsx` 替换为 `cargo build -p dsx-hp`（仅构建 HP）

---

## 六、架构示意图（合并后）

```
┌──────────────────────────────────────────────────┐
│ dsx-tauri (Tauri v2)                             │
│                                                   │
│  ┌──────────────┐   ┌──────────────────────────┐  │
│  │ Preact 前端  │   │ Rust 后端                 │  │
│  │  (WebView)   │   │                          │  │
│  │              │   │  Tauri Command Handlers   │  │
│  │  App.tsx     │◄──┤  mpsc::channel ──────┐   │  │
│  │  InfoPanel   │   │  agent worker thread  │   │  │
│  │  ChatMessage │   │    ↓                  │   │  │
│  │  Settings    │   │  dsx_agent::orchestrat│   │  │
│  │  Workspace   │   │  dsx_agent::session   │   │  │
│  └──────┬───────┘   │  dsx_agent::health    │   │  │
│         │           └──────────┬───────────────┘  │
│         │ Tauri IPC            │                   │
└─────────┼──────────────────────┼───────────────────┘
          │                      │
          │              ┌───────┴────────┐
          │              │  dsx-hp (进程)  │
          │              │  TCP localhost │
          │              └───────┬────────┘
          │                      │
          │              ┌───────┴────────┐
          │              │  dsx-tools     │
          │              │  (子进程)      │
          │              └────────────────┘
          │
    事件通道（13 个事件，通过 app.emit）
```

---

## 七、影响评估

| 指标 | 合并前 | 合并后 | 改善 |
|------|--------|--------|------|
| 后端 Rust 代码 | 653 行（含进程管理 ~120 行） | ~530 行 | -19% |
| 前端 TS/TSX | 1,717 行 | 1,717 行（不变） | 0% |
| 总编译产物 | dsx.exe (~15MB) + dsx-tauri | 仅 dsx-tauri | -50% |
| 消息延迟 | 序列化+反序列化+进程调度 | 直接函数调用 | ~1-5ms → ~0.01ms |
| agent 启动时间 | ~500ms（sleep）+ 进程启动 | 线程启动 | ~1ms |
| 错误可追溯性 | JSON 字符串解析 | Rust enum + Result | 大幅提升 |
| 构建流程复杂度 | 两步构建（dsx + tauri） | 一步构建 | 大幅降低 |
