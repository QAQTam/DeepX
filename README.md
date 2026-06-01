# DeepX v4.0

基于 DeepSeek-V4 的 Windows/Android 终端 AI 编程助手，1M-token 上下文，原生 DSML 工具调用。

## 特性

- **DeepSeek V4 深度适配** — 论文原版 DSML Tool Call 格式，Think Max 推理模式
- **1M-token 上下文** — KV cache 高效复用，跨轮推理链不中断
- **28+ 工具** — 文件读写、命令执行、Git 操作、网页搜索/抓取、Context7 文档查询、项目探索
- **跨平台** — Windows 原生 + Termux (Android) + Linux，纯 Rust 静态编译
- **F10 菜单** — 实时切换模型/effort/上下文限制/语言，持久化配置
- **TUI 界面** — ratatui 终端界面，工具调用实时展示，DSML 兼容计数

## 架构

| Crate | 职责 |
|-------|------|
| `dsx` | CLI 入口，配置向导 |
| `dsx-agent` | Agent 核心：会话管理、编排、工具解析、上下文组装 |
| `dsx-hp` | API 代理、流式响应解析、安全 gate |
| `dsx-tools` | 工具执行：exec / file / web / git / explore / plan / task |
| `dsx-tui` | 终端界面（ratatui + crossterm） |
| `dsx-types` | 共享类型：消息、配置、ToolCall、平台抽象 |
| `dsx-proto` | IPC 协议：UI↔Agent↔HP↔Tools 帧定义 |

### 数据流

```
TUI (ratatui)
  ↕ mpsc channel
Agent (编排)
  ↕ mpsc channel
HP (API 代理) → DeepSeek API
Tools (函数调用, 同进程)
```

## 快速开始

### 安装

```bash
# 从源码编译
git clone https://github.com/QAQTam/DeepX
cd DeepX
cargo build --release

# 或下载预编译二进制
# https://github.com/QAQTam/DeepX/releases
```

### 首次配置

```bash
dsx configure
# 输入 DeepSeek API key → 选择模型 → 完成
```

或直接启动 TUI，首次运行会自动进入配置向导：

```bash
dsx-tui
```

### 环境变量

| 变量 | 说明 |
|------|------|
| `DEEPSEEK_API_KEY` | API 密钥 |
| `DEEPSEEK_MODEL` | 模型名称（默认 `deepseek-v4-flash`） |
| `DEEPSEEK_EFFORT` | 推理强度：`high` / `max` |
| `DEEPSEEK_MAX_TOKENS` | 最大输出 token |

## 使用

### TUI 模式

```
dsx-tui
```

| 快捷键 | 功能 |
|--------|------|
| `F10` | 设置菜单（模型/effort/上下文/语言） |
| `F12` | Debug 面板 |
| `ESC` | 取消当前操作 |
| `Ctrl+C` | 退出 |

### Headless 模式

```bash
echo '{"type":"user_input","text":"帮我看下这个项目"}' | dsx
```

## 工具列表

| 分类 | 工具 | 说明 |
|------|------|------|
| 文件 | `read_file` `write_file` `edit_file` `glob` `grep` | 文件操作 |
| 执行 | `exec/run` | Shell 命令 |
| 探索 | `explore/scan` | 项目结构扫描 |
| Git | `git/status` `git/diff` `git/log` `git/commit` | 版本控制 |
| 网络 | `web/fetch` `web/search` `web/context7_resolve` `web/context7_query` | 网页与文档 |
| 任务 | `task/create` `task/update` `task/list` | 任务管理 |
| 计划 | `plan/create` `plan/update` `plan/read` | 项目规划 |
| 交互 | `ask_user` `commit` | 用户交互 |

## Android (Termux)

```bash
pkg install rust binutils
git clone https://github.com/QAQTam/DeepX
cd DeepX
cargo build --release
./target/release/dsx-tui
```

## 构建 Release

```bash
cargo build --release
# → target/release/dsx.exe     (~7 MB)
# → target/release/dsx-tui.exe (~2 MB)
```

Release profile 已配置：LTO + strip + opt-level=z + codegen-units=1。

## 开发

```bash
# 全量检查
cargo check --workspace

# 运行测试
cargo test --lib

# 发布版本
gh release create vX.X.X target/release/dsx.exe target/release/dsx-tui.exe
```

## 许可证

MIT
