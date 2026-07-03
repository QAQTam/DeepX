# DeepX 代码规范化审计 — 执行计划

**目标**：消除两个月多人开发累积的技术债，减行数、提稳定性、统一抽象层次。
**范围**：10 crate，86 文件，~23,600 行。
**原则**：改动越小越先做，低风险优于高风险，改完即编译验证。

---

## 阶段一：低风险清理（先行，建立信心）

### T1: 删除死文件 mcp_bridge.rs.1782801159
- 备份残留文件，lib.rs 未声明，完全无用
- 风险：无

### T2: 提取 msglp 辅助函数 → util.rs
- `format_tool_args_display`、`build_turns_from_context`、`build_compact_prompt`、
  `chrono_*`、`civil_from_days` 等纯函数移到 `util.rs`
- 文件：`crates/deepx-msglp/src/lib.rs:1556-1806`，约 250 行
- 风险：低（纯函数，无状态依赖）

### T3: 提取 msglp 通知系统 → notification.rs
- `NotificationThread`、Windows Toast 函数、`escape_xml`
- 文件：`crates/deepx-msglp/src/lib.rs:110-353`，约 250 行
- 风险：低（已有 `toast_com.rs` 在外面，耦合松）

### T4: 合并微工具文件（9 → 3）
- `file_write + file_edit + file_edit_diff + file_delete` → `file_mutate.rs`
- `file_read + file_list_dir + file_search + file_diff` → `file_query.rs`
- `file_move` → 并入 `file_mutate.rs`
- 风险：低（接口不变，仅移动代码）

---

## 阶段二：结构优化（去重复，减维护成本）

### T5: 消除文件工具内重复逻辑
- 统一 `file_write` 和 `file_edit` 的 diff 输出格式
- 统一二进制文件检测（一处引用）
- 合并 `exec_move_file` / `exec_copy_file`
- 风险：中（需验证所有工具仍正常工作）

### T6: Provider 数据从函数改为声明式
- `registry.rs` 中 8 个 `fn *_provider() → ProviderSpec` 改为 const 数组或 TOML
- 减 ~150 行
- 风险：中（需保证 `all_providers()` 返回值不变）

---

## 阶段三：跨 Crate 去重（影响最大，需设计）

### T7: 统一 daemon 连接抽象 → deepx_daemon::Client
- TUI (`terminalui/lib.rs`) 和 Tauri (`agent_bridge.rs`) 各自实现了 daemon 连接+自动启动
- 在 `deepx-daemon` 中提供 `Client::connect_with_auto_start(timeout)` 
- 两处调用方改为使用同一接口
- 风险：高（涉及两个入口的进程管理）

### T8: 统一 Agent 子进程 spawn 逻辑
- TUI 和 Tauri 各自实现了 agent child process spawn + stderr 日志线程
- 提取到 `deepx-daemon` 或共享模块
- 风险：高

### T9: 消除 `civil_from_days` 三份重复
- `msglp/lib.rs`、`agent_bridge.rs`×2 → 一处定义（`deepx-types` 或 `deepx-session`）
- 风险：低（纯算法函数）

### T10: `cmd_save_config` 16 参数改为 struct 解析
- `agent_bridge.rs:762` — 用 `#[derive(Deserialize)]` struct 替代 16 个 `if !value.is_empty()`
- 风险：低（Tauri 前端需配合变更 JSON key）

---

## 阶段四：收尾

### T11: 最终编译 + 回归验证
- `cargo check --workspace`
- `cargo test --workspace`
- 确认 TUI 和 Tauri 均能启动
