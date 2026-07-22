# DeepX

DeepX 是一个以 Rust Daemon 为核心、支持长时间运行任务和多客户端接入的本地 AI Agent 工作台。

它将会话、Agent、工具、配置和持久化放在独立的 `deepx-daemon` 中；图形界面由单独的 [deepx-desktop](https://github.com/QAQTam/deepx-desktop) 仓库提供。关闭 Desktop 不会终止 Daemon 或正在执行的 Agent 任务。

> 当前版本：0.9.0 架构迁移期。Windows 是主要交付平台；Linux/macOS 保持跨平台代码结构，并将在 1.0.0 前完成 Electron 打包与视觉验收。

## 架构

```text
┌──────────────────────┐
│ deepx-desktop        │  Electron + SolidJS
│ future deepx-tui     │  Ratatui + deepx-client
└──────────┬───────────┘
           │ authenticated loopback WebSocket
           │ versioned Control Protocol
┌──────────▼───────────┐
│ deepx-daemon         │  sessions / config / tools / leases / snapshots
└──────────┬───────────┘
           │ internal Ui2Agent / Agent2Ui protocol
┌──────────▼───────────┐
│ Agent workers        │  one worker per active session
└──────────────────────┘
```

`deepx-daemon` 是 `.deepx` 业务数据的唯一宿主。Desktop 和未来的 TUI 都不能绕过 Daemon 直接修改 Session、Config、Workspace 或 Plan 数据。

### 两个独立仓库

| 仓库 | 职责 |
| --- | --- |
| `QAQTam/DeepX` | Rust 后端、协议、Daemon、Agent、存储和 Rust Client |
| `QAQTam/deepx-desktop` | Electron Main/Preload、SolidJS UI、桌面打包和安装程序 |

两个仓库不通过 Git submodule 耦合。Desktop 使用 `deepx-backend.lock.json` 锁定后端版本、协议版本、Git commit、Release manifest 和 Daemon SHA-256，因此可以独立 Clone 和可复现打包。

## 主要能力

- 多 Session 会话、历史恢复和后台持续执行；
- 流式回答、Thinking、工具预览、终端输出和执行时间线；
- Permission、Ask、Plan 和用户交互恢复；
- Skills、Goal/Task、自动计划执行与控制；
- Workspace、Git Diff、变更审阅和文件操作；
- Settings、Provider、模型、Tokenizer、统计和 Token 遥测；
- Markdown、代码高亮、KaTeX、Diff 和图表展示；
- JSONL 与 Turso 持久化兼容、Compact 历史恢复；
- Daemon 单实例、发现、鉴权、心跳、租约、事件续传和 Snapshot；
- Daemon 与 Desktop 构建身份校验及安全升级接管。

## Workspace 结构

```text
crates/
├─ deepx-daemon       无界面的应用宿主和控制服务器
├─ deepx-runtime      与 GUI 无关的领域服务、事件总线和租约
├─ deepx-proto        Control Protocol 与 Agent 内部协议
├─ deepx-client       可复用的 Rust Daemon 客户端
├─ deepx-msglp        Agent 主循环和模型事件处理
├─ deepx-message      消息上下文与持久化投影
├─ deepx-session      Session 管理、JSONL/Turso 存储
├─ deepx-config       配置及数据库镜像
├─ deepx-tools        文件、Shell、Git、Plan 等工具
├─ deepx-skills       Skills 发现和运行时状态
├─ deepx-subagent     子 Agent 能力
├─ deepx-gate         模型访问与流式输入
├─ deepx-types        公共领域类型
└─ deepx-companion    暂停交付的 Companion 协议与源码
```

GUI 不在这个 Rust workspace 中。旧 Tauri 壳已在完成 Electron 功能对齐后移除，`deepx-desktop` 是唯一桌面前端和 UI 事实来源。

## 快速开始

### 只运行后端

需要 Rust stable 工具链。推荐安装 [just](https://github.com/casey/just)，也可以直接执行对应的 Cargo 命令。

```powershell
git clone https://github.com/QAQTam/DeepX.git
cd DeepX
cargo build -p deepx-daemon
cargo run -p deepx-daemon -- run
```

常用 Daemon 命令：

```powershell
cargo run -p deepx-daemon -- status
cargo run -p deepx-daemon -- stop
```

使用 `just`：

```powershell
just daemon
just dev
just status
just stop
```

### 本地联调 Desktop

建议将两个仓库放在同一父目录：

```text
D:\
├─ DeepX
└─ deepx-desktop
```

Desktop 使用 pnpm 11：

```powershell
git clone https://github.com/QAQTam/deepx-desktop.git D:\deepx-desktop
cd D:\deepx-desktop
pnpm install --frozen-lockfile
just dev ../DeepX
```

也可以分别启动：

```powershell
cd D:\DeepX
cargo build -p deepx-daemon

cd D:\deepx-desktop
$env:DEEPX_BACKEND_ROOT = 'D:\DeepX'
pnpm dev
```

## 构建与发布

### 后端 Release 制品

```powershell
just release-assets
```

该命令生成优化后的 `deepx-daemon`、平台制品、Release manifest 和 SHA256SUMS。正式 Release 应先发布这些文件，再更新 Desktop 的 `deepx-backend.lock.json`。

### Electron Windows 安装包

从 Desktop 锁定的 GitHub Release 下载并验证 Daemon：

```powershell
cd D:\deepx-desktop
pnpm package:win
```

使用本地后端源码打包：

```powershell
just package-local ../DeepX
```

安装包输出到 `deepx-desktop/release/`。Electron 安装包内包含 `deepx-daemon.exe`，但 Desktop 退出不会杀死 Daemon。

## 开发检查

后端：

```powershell
just check
just test
just fmt
just clippy
```

Desktop：

```powershell
cd D:\deepx-desktop
pnpm typecheck
pnpm test
pnpm build
```

修改 Control Protocol 时，必须同时验证：

1. `deepx-proto` 序列化往返与版本拒绝；
2. `deepx-runtime` Snapshot、事件顺序和租约；
3. `deepx-client` 发现、续传与重连；
4. Electron 请求关联、事件合批和断线恢复；
5. Desktop 锁文件中的协议版本和后端 Release 身份。

## Daemon 发现与安全边界

Daemon 只监听随机的 loopback 端口，并在用户数据目录写入发现文件：

- Windows：`%USERPROFILE%\.deepx\daemon.json`
- Linux/macOS：使用对应的 XDG/用户数据目录

发现文件包含 endpoint、PID、server epoch、协议版本和每次启动随机生成的 token。原生客户端读取该文件，在 WebSocket 握手时使用 Bearer token；Renderer 不接触 token，也不启用 Node.js。

同一 Session 使用独占控制租约：客户端每 5 秒心跳续租，断线后保留 15 秒恢复窗口。其他客户端仍可查看全局 Session 活动，但不能控制已被占用的 Session。

## 数据兼容性

0.9.0 不要求迁移或清空 `.deepx`。Daemon 继续兼容现有 Session、JSONL、Turso、Config、Workspace 和 Plan 数据。

升级、重装或卸载桌面壳时，不应默认删除用户的 `.deepx` 数据。任何清理用户数据的安装程序选项都必须显式说明并由用户选择。

## 0.9 → 1.0 路线

0.9.0 完成了进程与仓库分离。1.0.0 前的重点是稳定边界，而不是再次更换 GUI 技术栈：

- Electron 成为 Windows/Linux/macOS 唯一桌面实现；
- 固化 Control Protocol 兼容政策和跨仓库 Release CI；
- 完成安装、升级、卸载与 Daemon 生命周期端到端验证；
- 完成 Linux/macOS 的视觉和打包验收；
- 继续降低流式事件、长会话和多会话场景的内存占用；
- 以 `deepx-client` 为基础接入 Ratatui，不复制业务逻辑；
- 评估将暂停的 Companion 能力作为 Daemon 服务重新引入。

详细背景与技术选择见：

- [DeepX 0.9.0：从桌面程序走向多客户端平台](docs/releases/v0.9.0.md)
- [Daemon architecture](docs/daemon-architecture.md)
- [Desktop migration record](https://github.com/QAQTam/deepx-desktop/blob/main/MIGRATION.md)

## License

[MIT](LICENSE) © 2026 Sinyee
