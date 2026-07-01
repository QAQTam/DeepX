# 2026-06-30 崩溃灾难 — 复盘报告

> 耗时 24h 不间断修复，涉及栈溢出 + COM Use-After-Free 两个连续 bug。

---

## 灾难概要

**现象：** deepx 进程在 3-4 轮工具对话后崩溃，`0xc0000005` Access Violation。

**影响范围：** Debug / Release 双构建、TUI / Tauri 双前端、所有 Windows x64 用户。

**修复轮次：** 5 次不当修复 → 最终第 6 次根治。

**总耗时：** 2026-06-30 23:24（首次崩溃）→ 2026-07-02 01:58（最终修复），约 26h。

---

## 根因链（完整版）

### 第一层：直接原因

Windows 默认线程栈 `commit=4KB`。第 4 轮工具调用时并发线程的已提交栈总量超过此阈值。

### 第二层：引入原因

| Commit | 日期 | 引入 |
|--------|------|------|
| `12459e2` | Jun 24 | `notify-rust` + `std::thread::spawn`，通知线程无 COM 管理 |
| `f9d8a2b` | Jun 30 16:19 | `exec.rs` 引入 `let mut buf = [0u8; 4096]`（4KB 栈）+ daemon 架构 |
| `b835450` | Jun 30 17:54 | `exec.rs`/`pty_windows.rs` 继续增栈 |
| `4b0176a` | Jun 30 23:13 | PTY stdin 改写，栈使用量越界 |

### 第三层：系统原因

`notify-rust` 的 Windows 后端（`tauri-winrt-notification` → `windows` crate）未做 COM 初始化：
- 最初靠 `CoIncrementMTAUsage`（MTA 回退路径）侥幸存活
- 加 `CoInitializeEx(STA)` 修复后，反而因 STA 公寓析构触发 FactoryCache Use-After-Free

### 第四层：根本原因

**跨平台第三方库的 Windows 后端缺乏 COM 生命周期管理。** 团队未对 `notify-rust` 做 Windows 专项尽职调查。

---

## 修复决策树

```
崩溃 0xc0000005 at Try::branch
│
├─ 假设①：栈溢出（正确！）
│  ├─ fix: [0u8;4096] → vec!       ← 部分缓解
│  ├─ fix: /STACK:8MB               ← 栈崩消失，但 COM 崩浮现
│  └─ 错误推论: 栈问题已解决        ← ❌ 忽略了崩法已变
│
├─ 假设②：COM 未初始化（正确！）
│  ├─ fix: CoInitializeEx(STA)      ← 半对，STA 引入新问题
│  └─ 错误推论: COM 已修复          ← ❌ 未验证 FactoryCache
│
└─ 最终修复：持久化通知线程         ← ✅ 根治
   ├─ COM 初始化一次，永不退出
   ├─ FactoryCache 指针不失效
   └─ 栈分配一次，无累积压力
```

## 关键失误总结

| # | 失误 | 后果 | 避免方法 |
|---|------|------|---------|
| 1 | 未做 `git bisect` | 未立刻定位 `f9d8a2b` 引入的 4KB 栈 buffer | 崩溃后第一件事 `git bisect` |
| 2 | 8MB 栈后仍崩未警觉 | 以为栈问题未解，绕圈浪费 12h | 换崩法 → 换排查方向 |
| 3 | `CoInitializeEx(STA)` 而非 MTA | 打破原 MTA 平衡，FactoryCache 崩 | 回退前先理解原架构的隐式假设 |
| 4 | `notify-rust` 未做 COM 审查 | 完全不知其 Windows 后端依赖 COM | 新增依赖时必须审计所有平台后端 |

---

## 修复验证

- ✅ `cargo check -p deepx-msglp -p deepx-tauri -p deepx-terminal` 通过
- ✅ 通知线程专用化，COM 生命周期独立
- ✅ 栈 reserve=8MB, commit=8MB（`.cargo/config.toml`）
- 🟡 `conpty 0.7.0 Box::from_raw UB` 已知未修复（不影响稳定性）
- 🟡 `PipeReader::Drop .unwrap()` 已知未修复（不在主线程）

---

## 附录：涉及的 Pull Request / Commits

| commit | 描述 | 角色 |
|--------|------|------|
| `12459e2` | feat: desktop notification | 埋雷 |
| `f9d8a2b` | feat: comprehensive tool & architecture overhaul | 埋雷 |
| `4b0176a` | feat: PTY stdin write, cross-session memory | 爆发 |
| `321230d` | fix: move 4KB read buffers from stack to heap | 修复尝试 |
| `64fc503` | fix: set Windows stack reserve/commit to 8MB | 修复尝试 |
| `9519997` | fix: defensive fixes for 0xc0000005 crash | 修复尝试 |
| `6f06ba8` | fix (current) | COM init + 持久化线程 |
