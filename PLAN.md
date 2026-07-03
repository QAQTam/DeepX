# DeepX — 子进程加固计划

## 目标

确保 Tauri 前端无论如何不崩溃。Agent 崩了可自动拉起，前端崩了用户看到死窗口。

## 现状

| 已有 | 缺失 |
|------|------|
| panic hook 日志+flush | panic 后不 abort，进程可能僵死 |
| agent 意外退出检测 + 自动重 spawn | **无 agent 心跳检测**，只能等写入失败才发现 |
| 前端 Error 事件自动重连 | 重连仅匹配 "exited unexpectedly" 字符串，其他错误不触发 |
| daemon 重启上限 | **subagent 僵尸进程不回收** |
| agent loop catch_unwind | **Tauri 自身无 watchdog** |

---

## Phase 1: 最小加固（不改架构）

### T1: panic hook 加 abort
- 文件：`crates/deepx-tauri/src-tauri/src/main.rs`
- 改动：在 panic hook 末尾加 `std::process::abort()`
- 风险：无（崩了立刻死，好过僵死白屏）

### T2: agent 心跳检测
- 文件：`crates/deepx-tauri/src-tauri/src/agent_bridge.rs`
- 改动：AgentInstance 加心跳线程，每 10s `try_wait` 检查 agent 存活，死了自动 `ensure_agent` 拉起
- 风险：低（只读检查，不写）

### T3: Error 事件泛化
- 文件：`crates/deepx-tauri/src/App.tsx`
- 改动：所有包含 "exit"/"broken pipe"/"died" 的 Error 都触发重连，不靠精确匹配
- 风险：低

---

## Phase 2: 深度加固

### T4: subagent 僵尸回收
- 文件：`crates/deepx-tools/src/process_registry.rs`
- 改动：ProcessRegistry 加容量上限（最多 50 个），超限时自动 kill 最老的
- 风险：中（影响子代理并发上限）

### T5: 崩溃 dump 捕获
- 文件：`crates/deepx-tauri/src-tauri/src/main.rs`
- 改动：panic hook 写 crash.json 到 data_dir（含 timestamp、panic 消息、调用栈）
- 风险：低

### T6: Tauri window 关闭兜底
- 文件：`crates/deepx-tauri/src-tauri/src/lib.rs`
- 改动：已有 `WindowEvent::Destroyed → shutdown_all_agents()`，加超时保护：5s 内必须完成，否则 force-exit
- 风险：低

---

## 执行顺序

T1 → T2 → T3 → 编译验证 → commit
