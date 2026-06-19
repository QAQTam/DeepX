# DeepX 项目概览

> AI 驱动的桌面端编码助手，支持终端 UI 和原生桌面 GUI，连接 LLM API 实现自主编程。

---

## 项目结构

```
DeepX/
├── crates/
│   ├── deepx/              # 统一入口二进制 (main.rs)
│   ├── deepx-types/        # 共享类型定义（消息、会话、配置、提供者等）
│   ├── deepx-config/       # 配置加载/保存、提供者注册表、系统提示词
│   ├── deepx-gate/         # LLM API 流式网关 (HTTP SSE 客户端)
│   ├── deepx-msglp/        # 核心 Agent 事件循环
│   ├── deepx-message/      # 会话消息存储与状态机
│   ├── deepx-proto/        # IPC 协议 (JSON-LP)
│   ├── deepx-session/      # 会话持久化管理器
│   ├── deepx-tools/        # 工具执行框架（16 个工具）
│   ├── deepx-tui/          # 终端 UI (ratatui)
│   └── deepx-tauri/        # Tauri 桌面应用 (SolidJS 前端)
├── docs/                   # 设计文档
├── scripts/                # 辅助脚本
└── Cargo.toml              # 工作区定义
```

---

## 后端架构 (Rust)

### 核心设计模式

- **单一二进制多模式**：一个 `deepx` 二进制支持 TUI、Tauri GUI、Agent 子进程、CLI 四种模式
- **IPC over JSON-LP**：前端与 Agent 子进程通过 stdin/stdout 的 JSON-LP 通信，前端无关
- **轮次协议 (v5)**：每次用户输入产生 1~N 轮对话，每轮 = 一次 API 调用 + 工具执行
- **三阶段工具执行**：prepare → execute → finalize，支持并行工具执行
- **Effect 状态机**：MessageStore 操作返回 Effect 枚举，驱动循环决策
- **提供者适配层**：枚举驱动的多提供者适配（ThinkingParamMode、CacheTokenField 等）

### 分层职责

| 层级 | Crate | 职责 |
|------|-------|------|
| **类型层** | `deepx-types` | 所有共享数据结构，无运行时逻辑 |
| **配置层** | `deepx-config` | TOML 配置、8 个内置提供者注册、系统提示词 |
| **网络层** | `deepx-gate` | HTTP SSE 流式 API 客户端，支持重试与适配 |
| **协议层** | `deepx-proto` | UI↔Agent 双向 JSON-LP 协议定义 |
| **会话层** | `deepx-session` | TOML + SHA-256 校验的会话持久化 |
| **消息层** | `deepx-message` | 会话消息状态机（Turn/Step 模型），单数据源 |
| **循环层** | `deepx-msglp` | Agent 主循环：输入 → 网关 → 工具 → 输出 |
| **工具层** | `deepx-tools` | 16 个工具（exec/explore/read/write/edit/grep 等） |
| **UI 层** | `deepx-tui` | 终端前端 (ratatui + crossterm) |
| **桌面层** | `deepx-tauri` | Tauri v2 桌面壳，桥接 Agent 子进程 |

### 工具清单 (16 个)

exec, explore, web_fetch, read_file, write_file, edit_file, sed, grep, edit_file_diff, list_dir, search, delete_file, move_file, copy_file, glob, diff, task, ask_user

### 内置提供者 (8 个)

DeepSeek, Qwen, GLM, Kimi, MiMo, MiniMax, Doubao, OpenAI

---

## 前端架构 (SolidJS + Tauri v2)

### 技术栈

| 技术 | 用途 |
|------|------|
| SolidJS 1.9 | 响应式 UI 框架 |
| Vite 8 + rolldown | 构建工具 |
| TypeScript 6 | 类型系统 |
| Tailwind CSS v4 | 样式 |
| @kobalte/core | 无障碍 UI 组件 |
| @tanstack/solid-virtual | 虚拟化列表 |
| @tauri-apps/api | Tauri IPC |
| streaming-markdown | 流式 Markdown 渲染 |
| diff2html + ansi-to-html | Diff / ANSI 渲染 |

### 前端结构

```
src/
├── main.tsx                          # 入口
├── App.tsx                           # 根组件：侧边栏 + 主内容路由
├── App.css                           # Tailwind 导入 + 全局样式
├── store/
│   └── chat.ts                       # 聊天状态管理、流式事件处理
├── components/
│   ├── ChatView.tsx                  # 聊天布局 (InfoBar + MessageList + InputBar)
│   ├── MessageList.tsx               # 虚拟化消息列表
│   ├── MessageItem.tsx               # 单条消息渲染
│   ├── ThinkingBlock.tsx             # 可折叠思考块
│   ├── ToolCallCard.tsx              # 工具调用卡片 (ANSI 渲染)
│   ├── InputBar.tsx                  # 输入栏 (发送/停止)
│   ├── InfoBar.tsx                   # 模型信息、Token 用量、压缩按钮
│   ├── StatusPanel.tsx               # 浮动面板：任务、编辑记录、活动日志
│   ├── SettingsView.tsx              # 设置页：提供者/模型/API Key/语言
│   ├── AskDialog.tsx                 # 交互式用户询问弹窗
│   ├── MarkdownBody.tsx              # Markdown 渲染
│   ├── DiffBody.tsx                  # Diff 渲染
│   └── ...
├── i18n/
│   ├── index.ts                      # 国际化上下文（自动语言检测）
│   ├── en.ts                         # 英文
│   └── zh.ts                         # 中文
└── styles/
    └── markdown.css                  # Markdown 样式
```

### 前后端通信架构

```
SolidJS 前端
    │  invoke("cmd_*", args)
    ▼
Tauri Command (Rust) → JSON-LP → Agent 子进程 (stdin)
                                        │
    ▲                                  stdout
    │  Tauri Event ("agent-event")
    │
  监听 listen("agent-event", cb)
    │
    ▼
解析 Agent2Ui 帧 → 更新 store → 响应式渲染
```

### Tauri 命令 (15 个)

send_message, create_session, new_session, cancel, save_config, load_config, list_sessions, load_session, delete_session, resume_session, set_active_session, undo_turn, compact, load_more_turns, get/set_workspace, get_debug_snapshot

---

## 关键架构特点

1. **前后端解耦**：Agent 核心是纯 stdin/stdout 进程，可被任意前端驱动
2. **流式渲染**：代理批量化 Delta（~30ms 间隔），RoundComplete 原子替换确保一致
3. **会话完整性**：TOML + SHA-256 校验，抗损坏
4. **CJK 安全**：显式处理多字节字符，有专门的静态分析脚本
5. **工具安全**：三阶段锁定、超时控制、危险命令检测
6. **多提供者适配**：通过枚举字段抽象各 API 差异
