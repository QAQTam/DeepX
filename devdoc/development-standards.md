# DeepX 开发规范

> 基于 2026-06-30 崩溃灾难复盘制定的强制规则。

---

## 一、提交规范

### 1.1 提交信息格式

```
<type>: <简短描述>

[可选详细说明]
```

类型：

| 类型 | 何时使用 |
|------|---------|
| `feat` | 新功能 |
| `fix` | 修 bug |
| `chore` | 版本号、依赖、配置 |
| `refactor` | 重构，无功能变化 |
| `docs` | 文档 |
| `revert` | 回滚 |
| `perf` | 性能 |
| `security` | 安全 |

### 1.2 禁止的提交信息

- ❌ `fix` — 必须说明 fix 了什么
- ❌ `clean` / `clean code` / `cleanup` — 必须说明清理了什么
- ❌ 无正文的单行 fix — 必须关联 issue 或说明上下文

### 1.3 每次提交必须说明

- **WHAT**：改了哪些文件
- **WHY**：为什么改（关联 issue / 崩溃现象 / 架构决策）
- **RISK**：对 Windows / Linux / macOS 平台的影响

---

## 二、代码审查清单

### 2.1 线程安全

- [ ] `std::thread::spawn` 的新线程是否初始化了平台相关上下文？
  - Windows: `CoInitializeEx` 是否调用？（COM/WinRT 需要）
  - macOS: `dispatch_async` 主线程？`NSAutoreleasePool`？
- [ ] 瞬态线程是否有持久化替代方案？（channel + 工作线程优于每次 spawn）
- [ ] 线程退出时是否有资源泄漏？
- [ ] `catch_unwind` 是否覆盖了线程体？

### 2.2 平台安全

- [ ] 跨平台库的 Windows 后端是否已知 COM/WinRT 依赖？
  - 检查方式：`cargo tree` 查看是否依赖 `windows` / `winrt` 系列 crate
- [ ] 新依赖是否在非 Linux 上经过测试？
- [ ] 栈分配是否超过 1KB？→ 考虑 `vec!` 或 `Box::new`
- [ ] 文件路径处理是否使用 `std::path` 而非字符串拼接？

### 2.3 崩溃安全

- [ ] panic hook 是否已设？（`std::panic::set_hook`）
- [ ] 关键线程是否包裹了 `catch_unwind`？
- [ ] `unwrap()` / `expect()` 是否有替代方案？
  - 项目 clippy lint: `unwrap_used = "deny"`

### 2.4 Windows 专项

- [ ] `.cargo/config.toml` 的栈设置是否就位？
  ```
  rustflags = ["-C", "link-args=/STACK:0x800000,0x800000"]
  ```
- [ ] WinRT/COM 调用线程是否 `CoInitializeEx`？
- [ ] GUI 线程与工作线程的 COM 模型是否一致？（STA vs MTA）
- [ ] `windows` crate 的 `FactoryCache` 是否有可能跨公寓？

---

## 三、依赖管理

### 3.1 新增依赖流程

1. `cargo search <crate>` 查看最新版本
2. 检查 GitHub Issues 已知 bug（搜索 `windows`, `com`, `unsafe` 相关关键字）
3. 如果是跨平台 crate，检查其 Windows 实现是否依赖 COM/WinRT
4. 在 PR 描述中说明安全检查结论

### 3.2 需要警惕的 crate 类型

| 类型 | 风险 | 示例 |
|------|------|------|
| 通知库 | Windows 后端常依赖未文档化的 COM | `notify-rust`, `alert` |
| 系统托盘 | 通常走 WinRT | `tray-icon` |
| 剪贴板 | Windows 上走 COM | `arboard` |
| Shell 集成 | 可能依赖 COM | `open` |
| 音频 | 可能走 WinRT | `rodio` |

---

## 四、调试流程

### 4.1 崩溃发生后第一件事

1. **不要猜** — 看崩溃地址、看调用栈、看线程
2. **git bisect** — 定位首次引入崩溃的 commit
3. **区分触发条件与底层原因** — 比如"Done 事件触发"是触发条件，不是根因
4. **修改一个变量，验证一个结论** — 不要同时改多处

### 4.2 WinDbg 调试清单

```
# 基本信息
.exr -1       # 异常记录
k             # 调用栈
r             # 寄存器
!address rip  # 代码是否分页
!address <fault_addr>  # 故障地址状态

# 线程信息
~             # 所有线程
~*k           # 所有线程的栈
|             # 进程信息

# 符号
.sympath+ <path>   # 加 PDB 路径
.reload /f <module> # 强制重载符号

# 内存
!heap -s      # 堆状态
!address -f  # 完整内存布局
```
