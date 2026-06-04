# DeepX v4.2

DeepSeek-V4 AI 编程助手 — Tauri 桌面应用 / TUI 终端，1M-token 上下文，原生 DSML 工具调用。

## 特性

- **DeepSeek V4 深度适配** — 论文原版 DSML Tool Call 格式，Think Max 推理模式
- **1M-token 上下文** — KV cache 高效复用，跨轮推理链不中断
- **Tauri 桌面应用** — React 前端，流式增量渲染，工具调用卡片，工作区面板
- **TUI 终端** — ratatui 界面，工具调用实时展示，F10 即时设置
- **21+ 工具** — 文件读写、Shell 命令、Git 操作、网页搜索/抓取、Context7 文档查询、项目探索
- **跨平台** — Linux (.deb) + Windows，纯 Rust + TypeScript

## 架构

```
┌──────────────────────────────────────────────────┐
│  Tauri GUI (React/TypeScript + Rust)              │
│  ┌──────────┐  IPC   ┌──────────────────────────┐│
│  │ React UI │◄──────►│ Rust backend (lib.rs)     ││
│  │ (Vite)   │  JSON  │ spawns dsx subprocess     ││
│  └──────────┘        └─────────┬────────────────┘│
└────────────────────────────────┼──────────────────┘
                                 │ stdin/stdout (JSON lines)
┌────────────────────────────────┼──────────────────┐
│  Agent (dsx)                   │                   │
│  ┌─────────────────────────────▼───────────────┐  │
│  │ Agent (编排)                                 │  │
│  │  ↕ TCP                                       │  │
│  │ Gate (dsx-gate) → DeepSeek API              │  │
│  │ Tools (函数调用, 同进程)                       │  │
│  └─────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────┘
```

| Crate | 职责 |
|-------|------|
| `dsx` | CLI 入口 + agent 子进程，配置向导 |
| `dsx-agent` | Agent 核心：会话管理、上下文组装、工具编排、DSML 解析 |
| `dsx-gate` | API 代理：HTTP 流式响应、KV cache 追踪、安全沙箱 |
| `dsx-tools` | 工具执行：exec / file / web / git / explore / task / MCP |
| `dsx-tui` | 终端界面（ratatui + crossterm） |
| `dsx-tauri` | Tauri 桌面应用（React 前端 + Rust IPC 后端） |
| `dsx-types` | 共享类型：Message、UsageInfo、ToolCall、平台抽象 |
| `dsx-proto` | IPC 协议：Agent2Ui / Ui2Agent / HP 帧定义 |

## 快速开始

### 依赖

- **Rust** 1.77+
- **Node.js** 20+ / **pnpm**（仅 Tauri GUI）
- **系统库**：`libwebkit2gtk-4.1-dev` `libgtk-3-dev` `libssl-dev`（Linux）

```bash
# Ubuntu/Debian
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libssl-dev libayatana-appindicator3-dev
```

### 编译运行

```bash
git clone https://github.com/QAQTam/DeepX
cd DeepX

# TUI 终端模式
cargo run -p dsx-tui

# Tauri 桌面应用 (dev, 热重载)
just tauri-dev

# 打包 .deb
just tauri-pack
```

`just -l` 查看全部构建命令。

### 首次配置

首次运行自动进入配置向导，或通过 TUI 的 `F10` 设置菜单。需要 DeepSeek API Key（[platform.deepseek.com](https://platform.deepseek.com)）。

## 工具列表

| 分类 | 工具 |
|------|------|
| 文件 | `read_file` `write_file` `edit_file` `edit_file_diff` `delete_file` `move_file` `file_copy` |
| 查找 | `glob` `search` `list_dir` `diff` |
| 执行 | `exec` / `run` — Shell 命令，自适应输出截断，支持 timeout/cwd |
| 探索 | `explore` / `scan` — 项目结构扫描 |
| 网络 | `web_fetch` `web_search` `context7_resolve` `context7_query` |
| 任务 | `task_create` `task_update` `task_list` |
| 交互 | `ask_user` |

## 快捷键 (TUI)

| 键 | 功能 |
|----|------|
| `F10` | 设置菜单（模型 / effort / 上下文限制 / 语言） |
| `F12` | Debug 面板 |
| `ESC` | 取消当前操作 |
| `Ctrl+C` | 退出 |

## Android (Termux)

```bash
pkg install rust binutils
git clone https://github.com/QAQTam/DeepX
cd DeepX
cargo build --release -p dsx-tui
./target/release/dsx-tui
```

## CLI / Headless

```bash
# 输入 JSON 行 → 输出 JSON 行
echo '{"type":"user_input","text":"讲解这个项目架构"}' | dsx agent
```

## 许可证

MIT
