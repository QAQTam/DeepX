# DeepX 0xc0000005 Access Violation — 灾难恢复报告

> 最终更新: 2026-07-02 | 涉及版本: v0.5.0 | 修复状态: ✅ 已修复

---

## 一、崩溃现象

Windows x64 上 `deepx`（后拆分为 `deepx-tauri.exe` / `deepx-terminal.exe`）在 **3-4 轮工具密集型对话后，LLM 返回 Done 事件时** 崩溃，异常码 `0xc0000005`（STATUS_ACCESS_VIOLATION）。Debug / Release、TUI / Tauri 前端均复现。

---

## 二、崩溃演化史

### 阶段 A：栈提交耗尽（首次崩溃）

| 项目 | 数值 |
|------|------|
| 时间 | 2026-06-30 23:24:44 |
| 异常 | `0xc0000005` — 写栈越界 |
| RIP | `Try::branch` (`mov [rsp+8], rcx`) |
| 默认栈 | reserve=1MB, commit=**4KB** |

**根因：** Windows 默认线程栈 commit=4KB（仅一个页面）。第 4 轮工具调用时：
- 多个工具线程并发（exec 等），每线程栈帧含 4KB buffer
- 通知线程也在第 4 轮 Done 事件时启动
- 并发线程的已提交栈总量超过 4KB → 访问未提交的栈页 → AV

**触发链：**
```
埋雷①: 12459e2 (Jun 24) 引入 notify-rust + std::thread::spawn
埋雷②: f9d8a2b (Jun 30 16:19) exec 重写 → let mut buf = [0u8; 4096]
埋雷③: b835450 (Jun 30 17:54) + 4b0176a (Jun 30 23:13) exec/pty 继续加压
爆发:   4 轮后并发栈总量 > 4KB → Try::branch 写栈崩
```

### 阶段 B：栈扩容后 → COM Use-After-Free 浮现

| 修复 | 效果 |
|------|------|
| `[0u8;4096]` → `vec!` (堆) | 减少 4KB 栈，但多线程并发下不够 |
| `/STACK:0x800000,0x800000` (8MB) | 栈溢出消失，**但露出了第二个 bug** |
| `CoInitializeEx(COINIT_APARTMENTTHREADED)` | 修复 COM 未初始化，但 MTA → STA 切换引入新问题 |

**新崩路径：**
```
notify-rust 瞬态线程
→ CoInitializeEx(STA)
→ FactoryCache 缓存 COM 工厂指针
→ 线程退出 → STA 公寓析构 → COM 底层释放工厂对象
→ 下次通知 → FactoryCache 读悬垂指针 → AV at generic_factory.rs:18
```

### 阶段 C：最终修复

**方法：** 用 `NotificationThread` 持久化线程替代 `std::thread::spawn` 瞬态线程。

```rust
struct NotificationThread {
    tx: mpsc::Sender<String>,
    _thread: JoinHandle<()>,
}

impl NotificationThread {
    fn spawn() {
        // COM 初始化一次，线程永不退出
        CoInitializeEx(COINIT_APARTMENTTHREADED);
        loop { recv() → notify_rust::show(); }
    }
}
```

**原理：**
- COM 在同一线程上持续存活，FactoryCache 指针不失效
- 栈只分配一次，无累积压力
- 瞬态线程退出导致的 STA 析构问题不复存在

---

## 三、原始 debug.md 假设 vs 真相

| 原始假设 | 实际真相 | 判断 |
|---------|---------|------|
| 栈 reserve 不足 (1MB) | 栈 commit 不足 (4KB) 才是主因 | 半对 |
| 8MB 栈应修复 | 崩#4 发生在 8MB 后 → 实际已转为 COM 崩 | ❌ 未识别 |
| `Try::branch` 是唯一根因 | 只是第一阶段；第二阶段是 FactoryCache UAF | ❌ 未预见 |
| 栈上 4KB buffer 是主因 | 是诱因，但即使移到堆，多线程并发 4KB commit 仍耗尽 | 半对 |
| 未分析 COM 公寓模型 | MTA vs STA 切换是第二阶段崩溃的直接原因 | ❌ 完全缺失 |
| 未追溯 notify-rust 引入时间 | `12459e2` (Jun 24) 埋下第一颗雷 | ❌ 缺失 |

---

## 四、灾难恢复流程（复盘）

### 4.1 时间线

```
Jun 24 03:06  12459e2   埋雷: notify-rust 引入
Jun 30 16:19  f9d8a2b   埋雷: exec 架构大改 (4KB 栈 buffer)
Jun 30 17:54  b835450   埋雷: exec/pty 继续增栈
Jun 30 23:13  4b0176a   爆发: PTY stdin 改 → 栈阈值越过
Jun 30 23:14  2a25740   bump v0.5.0
Jun 30 23:24            ★ 首次崩溃
Jul  1 00:18-02:04      多次尝试修复 (堆分配/catch_unwind/8MB栈)
Jul  1 02:04-15:32      CoInitializeEx(STA) 加入 → 崩法变了
Jul  2 01:58 ★          持久化线程 → 根治
```

### 4.2 关键失误

1. **未做 git bisect** — 最初崩溃后未立即定位 `f9d8a2b` 引入的 `[0u8;4096]`
2. **8MB 栈后仍崩未警觉** — 未意识到已转为 COM 崩，仍在栈方向绕圈
3. **CoInitializeEx 用了 STA 而非 MTA** — 原代码靠 `CoIncrementMTAUsage` 走 MTA 无事，主动 STA 反而打破了原平衡
4. **notify-rust 未做 Windows COM 尽职调查** — 不知其 Windows 后端无 COM 管理

---

## 五、经验教训

1. **Windows 上所有 WinRT/COM 调用必须显式初始化 COM**，不能依赖第三方库的内部回退路径
2. **瞬态线程 + COM = 危险组合**：STA 模式下线程退出会销毁公寓
3. **默认 4KB 栈 commit 是 Windows 陷阱** — Rust 多线程应用必须主动设 `reserve + commit`
4. **崩溃地址不变 ≠ 根因不变** — 两个不同 bug 可能 RIP 到同一函数不同偏移
5. **`forget()` 不保证 COM 对象存活** — Rust 侧防了 Release，COM 内部公寓层仍有自己的生命周期
