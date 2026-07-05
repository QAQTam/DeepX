# Phase 7 新增依赖文档

> 来源: Context7 API docs, 2026-07-05

---

## 1. `turso` — Turso Database 引擎 Rust crate

**用途:** Phase 7.10 Session 双库（本地 .db 后端）
**添加位置:** `crates/deepx-session/Cargo.toml`
**版本目标:** 0.12+

```toml
[dependencies]
turso = { version = "0.12", optional = true }
tokio = { version = "1", features = ["rt"], optional = true }

[features]
turso-backend = ["dep:turso", "dep:tokio"]
```

### API 摘要

```rust
// 打开本地数据库
let db = turso::Builder::new_local("sessions.db").build().await?;
let conn = db.connect()?;

// 执行 DDL
conn.execute_batch("CREATE TABLE IF NOT EXISTS ...").await?;

// 插入
conn.execute("INSERT INTO users (name) VALUES (?1)", ["Alice"]).await?;

// 查询
let mut rows = conn.query("SELECT * FROM users WHERE age > ?1", [18]).await?;
while let Some(row) = rows.next().await? {
    let id = row.get_value(0)?;
    let name = row.get_value(1)?;
}

// Prepared statement
let stmt = conn.prepare("INSERT INTO users (name) VALUES (?1)").await?;
// 或缓存版
let stmt = conn.prepare_cached("INSERT INTO users (name) VALUES (?1)").await?;
```

### async → sync 桥接

```rust
use tokio::runtime::Runtime;

pub fn open_sync(path: &str) -> Result<turso::Database, turso::Error> {
    let rt = Runtime::new().unwrap();
    rt.block_on(turso::Builder::new_local(path).build())
}
```

---

## 2. `sha2` — SHA-256 哈希 (RustCrypto)

**用途:** Phase 7.1 审计（args_hash 指纹）、Phase 7.9 exec 命令审计
**添加位置:** `crates/deepx-tools/Cargo.toml`
**状态:** 已是传递依赖（Cargo.lock 存在），需显式添加到 `[dependencies]`

```toml
[dependencies]
sha2 = "0.10"
```

### API 摘要

```rust
use sha2::{Sha256, Digest};

// 一次调用
let hash = Sha256::digest(b"hello world");
// → GenericArray<u8, U32>

// 增量
let mut hasher = Sha256::new();
hasher.update(b"hello ");
hasher.update(b"world");
let hash = hasher.finalize();

// Hex 编码
let hex_str = format!("{:x}", hash);  // 小写
// 或: base16ct::lower::encode_string(&hash)
```

---

## 3. `hex` — 十六进制编解码

**用途:** Phase 7.1 audit args_hash 的 hex 编码
**添加位置:** `crates/deepx-tools/Cargo.toml`
**状态:** 已是传递依赖，需显式添加

```toml
[dependencies]
hex = "0.4"
```

### API 摘要

```rust
// 编码
assert_eq!(hex::encode(b"Hello world!"), "48656c6c6f20776f726c6421");
assert_eq!(hex::encode(vec![1, 2, 3, 15, 16]), "0102030f10");

// 解码
assert_eq!(hex::decode("48656c6c6f"), Ok(b"Hello".to_vec()));

// 无堆分配的 slice 解码
let mut bytes = [0u8; 4];
hex::decode_to_slice("6b697769", &mut bytes).unwrap();
```

**注意:** `sha2` 的 `finalize()` 返回的 `GenericArray` 可以直接 `format!("{:x}", hash)`，不一定需要 `hex` crate。如果需要与 JSONL 中的其他 hex 字符串一致，可加。

---

## 4. `windows` — Windows API 绑定

**用途:** Phase 7.2 OS PIN 授权（CredUI 对话框）
**添加位置:** `crates/deepx-tools/Cargo.toml`
**需要 feature:** `Win32_Security_Credentials`, `Win32_UI_Shell`

```toml
[target.'cfg(windows)'.dependencies]
windows = { version = "0.58", features = [
    "Win32_Security_Credentials",
    "Win32_UI_Shell",
    "Win32_Foundation",
    "Win32_System_Threading",
] }
```

### API 摘要

```rust
use windows::Win32::Security::Credentials::{
    CredUIPromptForWindowsCredentialsW,
    CREDUIWIN_AUTO_PACK_CREDENTIAL, // 实际 flag 名需以 docs.rs 为准
};
use windows::Win32::Foundation::HWND;
use windows::core::PWSTR;

// 弹框获取 Windows 凭据
let mut username = PWSTR::null();
let mut password = PWSTR::null();
let mut save = false;

unsafe {
    CredUIPromptForWindowsCredentialsW(
        None,           // hwndParent
        0,              // authError
        &mut 0,         // authPackage (返回)
        None,           // inAuthBuffer
        0,              // inAuthBufferSize
        &mut username,  // outAuthBuffer (username)
        &mut 0,         // ulOutAuthBufferSize
        &mut save,      // pfSave
        CREDUIWIN_AUTO_PACK_CREDENTIAL,
    )?;
}
```

**↑ 具体 flag 名和签名以 [`windows` crate docs](https://microsoft.github.io/windows-docs-rs/doc/windows/) 为准。**

### Windows CredUI 备选: 简易 PIN 输入框

如果 CredUI 太重（需要 `winspool.drv` 等额外 DLL），可以用 `MessageBoxW` + 公开文本密码框，或直接用 `rpassword` crate 的 Windows 支持。

---

## 5. PAM (Linux) — 可插拔认证模块

**用途:** Phase 7.2 Linux PIN 授权
**添加位置:** `crates/deepx-tools/Cargo.toml`
**Crate:** `pam` 或 `pam-sys`

```toml
[target.'cfg(not(windows))'.dependencies]
pam = "0.7"
```

### API 摘要

```rust
use pam::{Authenticator, PasswordConv};

let mut auth = Authenticator::with_password("login").unwrap();
auth.get_handler().set_credentials("username", "password");
match auth.authenticate() {
    Ok(()) => println!("Authenticated"),
    Err(e) => eprintln!("Auth failed: {e}"),
}
```

**回退方案 (headless/SSH):**
```rust
// 读取 <data_dir>/pin_token 文件，比较 SHA-256
let stored = std::fs::read_to_string(pin_path)?;
let input_hash = Sha256::digest(input.as_bytes());
let expected = hex::decode(&stored.trim())?;
if input_hash.as_slice() == expected.as_slice() {
    Ok(())
} else {
    Err("PIN mismatch")
}
```

---

## 6. `tokio` — 异步运行时

**用途:** Phase 7.10 turso crate 的 async→sync 桥接
**添加位置:** `crates/deepx-session/Cargo.toml`
**状态:** 已是其他 crate 的传递依赖，需显式添加

```toml
[dependencies]
tokio = { version = "1", features = ["rt"], optional = true }
```

### 桥接模式

```rust
use std::sync::LazyLock;
use tokio::runtime::Runtime;

// 单例 Runtime（每次调用复用，避免 Runtime::new() 开销）
static RT: LazyLock<Runtime> = LazyLock::new(|| Runtime::new().unwrap());

pub fn block_on<F: std::future::Future>(f: F) -> F::Output {
    RT.block_on(f)
}
```

---

## 依赖概览

| Crate | Phase | 已有? | 需添加? |
|-------|-------|-------|---------|
| `turso` | 7.10 | ❌ | ✅ deepx-session (optional) |
| `tokio` | 7.10 | 传递 | ✅ deepx-session (optional) |
| `sha2` | 7.1, 7.9 | 传递 | ✅ deepx-tools |
| `hex` | 7.1 | 传递 | ⚠️ 可选（可用 `format!("{:x}")` 替代） |
| `windows` | 7.2 | ❌ | ✅ deepx-tools (cfg(windows)) |
| `pam` | 7.2 | ❌ | ✅ deepx-tools (cfg(not(windows))) |
