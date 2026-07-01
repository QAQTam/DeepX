# Windows COM 编程规则

> 本次崩溃灾难的核心教训。所有 Windows 平台开发者必须熟读。

---

## 一、COM 初始化基础

### 1.1 每个需要 COM 的线程都必须初始化

```rust
// 正确：每个线程初始化 COM
std::thread::spawn(|| {
    #[cfg(windows)]
    unsafe { let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED); }
    // ... 使用 WinRT/COM API
    #[cfg(windows)]
    unsafe { CoUninitialize(); }
});
```

```rust
// 错误：依赖跨平台库的内部回退
std::thread::spawn(|| {
    notify_rust::Notification::new().show();
});
```

### 1.2 STA vs MTA

| 模型 | 初始化方式 | 线程退出行为 | 适用场景 |
|------|-----------|-------------|---------|
| STA | CoInitializeEx(COINIT_APARTMENTTHREADED) | 公寓析构，释放关联对象 | UI 线程、WinRT 组件 |
| MTA | CoInitializeEx(COINIT_MULTITHREADED) | 无析构，线程安全 | 后台工作线程 |
| 无 | CoIncrementMTAUsage（隐式） | 无析构 | windows crate 回退路径 |

**规则：** 瞬态工作线程必须用 MTA，持久化专用线程可用 STA。绝不依赖隐式回退。

### 1.3 FactoryCache 陷阱

windows crate 的 FactoryCache 是一个函数级 static，缓存 COM 工厂指针。第一调用者填充缓存后，如果填充者线程退出时 STA 析构，COM 底层释放工厂对象，但 FactoryCache 的 AtomicPtr 仍然指向已释放内存。下次任何线程使用缓存就触发 Use-After-Free。

**规则：** FactoryCache 必须在持久化线程上填充，或者确保 COM 公寓永不被析构。

---

## 二、瞬态线程 vs 持久化线程

### 2.1 决策矩阵

| 条件 | 推荐方案 |
|------|---------|
| 只需要执行一次操作 | 持久化线程 + channel |
| 需要并发执行大量操作 | 线程池 + MTA 初始化 |
| 必须 STA（UI 组件） | 专用 STA 线程 + channel |
| 操作极轻量且确定无 COM | 瞬态线程 OK |

### 2.2 模式：Channel 持久化线程

```rust
struct Worker {
    tx: mpsc::Sender<Task>,
    _thread: JoinHandle<()>,
}

impl Worker {
    fn spawn() -> Self {
        let (tx, rx) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("deepx-worker".into())
            .spawn(move || {
                CoInitializeEx(COINIT_APARTMENTTHREADED);
                while let Ok(task) = rx.recv() {
                    process(task);
                }
                CoUninitialize();
            })
            .expect("spawn worker");
        Self { tx, _thread: thread }
    }
}
```

---

## 三、第三方库 Windows 后端审查

引入任何可能涉及 Windows 通知/UI/Shell/音频/剪贴板的库时：

1. cargo tree 检查是否依赖 windows / winrt / com 系列 crate
2. 搜索 GitHub Issues: COM, CoInitialize, thread safety, apartment
3. 检查 Windows 源码是否含 CoInitialize / RoGetActivationFactory / CoCreateInstance
4. 检查 README 是否提及 Windows COM 要求
5. 验证测试：从新线程调用时是否崩溃

---

## 四、Windows 栈配置

### 4.1 默认值陷阱

Windows 默认线程栈 commit=4KB（仅 1 页）。任何多线程应用只要 2 个线程同时忙就会越界。建议 reserve=8MB, commit=8MB。

### 4.2 配置方式

.cargo/config.toml:
```
[target.x86_64-pc-windows-msvc]
rustflags = ["-C", "link-args=/STACK:0x800000,0x800000"]
```

用 dumpbin /headers 验证。

---

## 五、forget() 的局限性

forget() 只阻止 Rust 侧的 Drop（即 Release 调用）。COM 对象还有公寓层的引用：STA 析构时 COM 内部可能释放对象。

**规则：** 永远不要靠 forget() + 跨公寓缓存来管理 COM 生命周期。确保 COM 公寓生存期超过缓存生存期。
