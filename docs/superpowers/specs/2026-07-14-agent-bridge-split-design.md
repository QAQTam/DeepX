# Agent Bridge 模块拆分设计

> **日期**: 2026-07-14
> **状态**: 设计中 → 待用户审阅
> **范围**: `crates/deepx-tauri/src-tauri/src/agent_bridge.rs`

---

## 1. 动机

`agent_bridge.rs` 当前为单文件 2170 行，包含 59 个顶层函数/结构体。主要痛点是定位函数耗时——38 个 `cmd_*` 函数与 agent 生命周期、平台检测、工具函数混在一起，缺乏清晰的模块边界。

## 2. 目标

1. **每个子模块 ≤ 300 行**：职责单一，打开文件即理解
2. **公开 API 零变更**：所有 `pub fn` 签名、`#[tauri::command]` 属性不变
3. **模块边界持久**：新增 Tauri command 时开发者清楚该放哪个文件

## 3. 目标文件结构

```
crates/deepx-tauri/src-tauri/src/
├── agent_bridge.rs → agent_bridge/mod.rs   # 原文件替换为目录
└── agent_bridge/
    ├── mod.rs                  # re-export 所有公开 API，零逻辑
    ├── platform.rs             # OS 检测、PATH 缓存、工具链检测 (~250行)
    ├── registry.rs             # Agent 子进程生命周期管理 (~280行)
    ├── util.rs                 # 零依赖工具函数 (~150行)
    └── commands/
        ├── mod.rs              # re-export (~15行)
        ├── session.rs          # 会话生命周期 + 消息 + 模式 (~280行)
        ├── permission.rs       # 权限弹窗 + ask_user 回复 (~120行)
        ├── git.rs              # Git 操作 (~250行)
        ├── config.rs           # 配置、工具列表、技能、工作区 (~200行)
        └── plan.rs             # Plan/Task + 统计/持久化 (~200行)
```

## 4. 模块职责边界

### 4.1 `mod.rs`
- **职责**: 仅 re-export。使用 `pub use` 不加前缀，保持外部调用路径不变
- **公开**: `platform::*`, `registry::*`, `commands::*`
- **依赖**: 所有子模块

### 4.2 `platform.rs`
- **职责**: 启动时一次性环境检测
- **公开 API**: `cache_system_path()`, `detect_os_info()`
- **内部**: `windows_reg_path()`, `reg_read_string()`, `reg_read_dword()`, `windows_os_info()`, `unix_os_info()`, `detect_tools()` (含 `try_version`)
- **依赖**: 无（仅 std + winapi FFI）
- **预估行数**: ~250

### 4.3 `registry.rs`
- **职责**: Agent 子进程的注册、启动、stdin/stdout 通信、关闭
- **公开 API**: `AgentInstance`, `AgentRegistry` + 全部 impl 方法, `shutdown_all_agents()`
- **内部**: `spawn_agent_process()`, `ensure_agent()`, `send_to_agent()`
- **依赖**: `platform`（读 `SYSTEM_PATH`）, `deepx_proto`（`Ui2Agent`/`Agent2Ui` frame）, `tauri`（`AppHandle`, `Emitter`）
- **预估行数**: ~280

### 4.4 `commands/session.rs`
- **职责**: 会话生命周期 + 用户消息 + 模式切换 + Dashboard
- **公开 API**: `cmd_send_message`, `cmd_new_session`, `cmd_resume_session`, `cmd_close_session`, `cmd_cancel`, `cmd_set_mode`, `cmd_get_dashboard_data`, `cmd_get_activity`, `cmd_load_more_turns`, `cmd_undo_turn`, `cmd_compact`
- **依赖**: `registry`（send_to）, `util`（read_file_preview）
- **预估行数**: ~280

### 4.5 `commands/permission.rs`
- **职责**: 权限弹窗 + ask_user 回复
- **公开 API**: `cmd_permission_response`, `cmd_ask_response`, `cmd_ask_dismiss`
- **依赖**: `registry`
- **预估行数**: ~120

### 4.6 `commands/git.rs`
- **职责**: Git 操作命令
- **公开 API**: `cmd_get_git_diff`, `cmd_get_git_file_diff`, `cmd_get_git_branch`, `cmd_list_branches`, `cmd_switch_branch`, `cmd_git_commit`
- **依赖**: `registry`
- **预估行数**: ~250

### 4.7 `commands/config.rs`
- **职责**: 配置 CRUD、工具列表、技能管理、工作区、会话列表
- **公开 API**: `cmd_save_config`, `cmd_load_config`, `cmd_get_version`, `cmd_list_available_tools`, `cmd_activate_skill`, `cmd_unload_skill`, `cmd_reload_skills`, `cmd_get_workspace`, `cmd_set_workspace`, `cmd_list_sessions`, `cmd_delete_session`
- **依赖**: `registry`
- **预估行数**: ~200

### 4.8 `commands/plan.rs`
- **职责**: Plan 解析/Task 操作 + Token 统计 + 迁移
- **公开 API**: `cmd_read_plan`, `cmd_plan_action`, `cmd_task_action`, `cmd_get_context_stats`, `cmd_get_token_stats`, `cmd_migration_count`, `cmd_migrate_to_turso`
- **依赖**: `registry`, `util`
- **预估行数**: ~200

### 4.9 `util.rs`
- **职责**: 零依赖的纯工具函数
- **公开 API**: `read_file_preview()`
- **内部**: `chrono_local_date_from_epoch()`, `generate_date_range()`, `days_before_today()`, `resolve_deepx_dir()`, `parse_plan_items()`, `agent2ui_event_name_for_ui()`
- **依赖**: 无（仅 std）
- **预估行数**: ~150

## 5. 依赖图（无环）

```
mod.rs ──→ commands/mod.rs ──→ commands/session.rs ──→ registry ──→ platform
                            ├── commands/permission.rs ──→ registry
                            ├── commands/git.rs ──→ registry
                            ├── commands/config.rs ──→ registry
                            └── commands/plan.rs ──→ registry, util

util.rs   ← 零依赖
platform  ← 零依赖
```

依赖方向严格单向，无循环。

## 6. 执行策略

### 6.1 搬迁顺序（叶子→根）
```
1. util.rs           ← 零依赖
2. platform.rs       ← 零依赖
3. registry.rs       ← 依赖 platform
4. commands/mod.rs   ← 空壳
5. commands/session.rs
6. commands/permission.rs
7. commands/git.rs
8. commands/config.rs
9. commands/plan.rs
10. mod.rs           ← re-export 收口
```

### 6.2 新旧并存过渡
- 原文件重命名为 `agent_bridge_legacy.rs`（不删除）
- 每一步搬迁后 `cargo check -p deepx-tauri`
- 全部验证通过后删除 legacy 文件

### 6.3 每步验证
```bash
cargo check -p deepx-tauri
grep -rn "旧路径" crates/deepx-tauri/
```

## 7. 不变量（不可触碰）

- ❌ 所有 `pub fn` 签名一字不改
- ❌ 所有 `#[tauri::command]` 属性保持原样
- ❌ 不触及其他 crate
- ❌ 不改函数逻辑（纯搬迁）
- ❌ 不修改 `Cargo.toml` 或 `build.rs`

## 8. 风险与回滚

| 风险 | 缓解 |
|---|---|
| re-export 路径断裂 | `cargo check` 立即捕获 |
| Tauri command 注册失效 | 函数名+签名不变 |
| `cfg(target_os)` 遗漏 | platform.rs 整体搬迁，cfg 原样保留 |
| 内部 `use` 遗漏 | `cargo check` 自动捕获 |

回滚：每个子模块搬迁后独立 commit，`git reset --hard` 到上一绿色 commit 即可。

## 9. 成功标准

- [ ] `cargo check -p deepx-tauri` 通过
- [ ] 每个子模块文件 ≤ 300 行
- [ ] `codegraph callers` 确认所有公开 API 引用路径不变
- [ ] `grep -rn "agent_bridge_legacy"` 无残留引用
- [ ] 新增一个 Tauri command 时，不在已有的 9 个文件中犹豫超过 5 秒
