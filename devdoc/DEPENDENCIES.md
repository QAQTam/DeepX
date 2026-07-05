# Phase 7 新增依赖文档

> 来源: Context7 API docs, 2026-07-05

---

---

## 0. `agentfs` — AgentFS Rust SDK (Turso)

**用途:** Phase 7.6 文件沙箱 + 审计（替换自建 audit.jsonl）
**添加位置:** `crates/deepx-tools/Cargo.toml`
**版本目标:** 0.1

```toml
[dependencies]
agentfs = "0.1"
tokio = { version = "1", features = ["rt"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
anyhow = "1"
```

### 核心 API

```rust
use agentfs::{AgentFS, AgentFSOptions};

// 持久化存储（.agentfs/{id}.db 单个 SQLite 文件）
let agent = AgentFS::open(AgentFSOptions::with_id("deepx-session")).await?;

// 临时内存数据库
let agent = AgentFS::open(AgentFSOptions::ephemeral()).await?;
```

### 三大子系统

#### 1. 键值存储 (`agent.kv`)

```rust
agent.kv.set("key", "value").await?;
let val: Option<String> = agent.kv.get("key").await?;
agent.kv.delete("key").await?;

// 结构化数据
#[derive(Serialize, Deserialize)]
struct UserPrefs { theme: String, language: String }
agent.kv.set("user:prefs", UserPrefs { ... }).await?;

// 列表（前缀过滤 + 分页）
let keys = agent.kv.list(ListOptions { prefix: Some("user:".into()), ..Default::default() }).await?;
agent.kv.clear().await?;
```

#### 2. 文件系统 (`agent.fs`) — POSIX-like, CoW 隔离

```rust
// 读写
agent.fs.write_file("/report.md", b"# Report\n...").await?;
let data = agent.fs.read_file("/report.md").await?;
let text = String::from_utf8(data)?;

// 目录操作
agent.fs.mkdir("/reports").await?;
let entries = agent.fs.readdir("/reports").await?;

// 元数据
let stat = agent.fs.stat("/report.md").await?;
let exists = agent.fs.exists("/report.md").await?;

// 删除 / 重命名 / 复制
agent.fs.rm("/old.txt").await?;
agent.fs.rename("/a.txt", "/b.txt").await?;
agent.fs.copy_file("/src.txt", "/dest.txt").await?;
```

#### 3. 工具调用追踪 (`agent.tools`)

```rust
agent.tools.record(tool_call).await?;
let calls = agent.tools.list(ToolListOptions::default()).await?;
let call = agent.tools.get("tool-id").await?;
agent.tools.delete("tool-id").await?;
```

### 审计能力

AgentFS 自动记录每个文件操作和工具调用到内部 SQLite，提供：

| 命令 | 作用 |
|------|------|
| `agentfs timeline <session>` | 时间线（所有操作） |
| `agentfs timeline --status error` | 只看失败 |
| `agentfs fs ls <session>` | 文件列表 |
| `agentfs fs cat <session> /path` | 读文件 |
| `agentfs diff <session>` | 差异对比（相对原文件系统） |
| SQL 查询 `.agentfs/*.db` | 自定义审计 |

### DeepX 集成方案

DeepX 当前的文件操作（`file_mutate.rs`, `file_query.rs`）通过 `std::fs` 直接操作。
AgentFS 替代方案：

```rust
// 替换前 (file_mutate.rs)
std::fs::write(path, content)?;

// 替换后 (AgentFS)
let agent = AgentFS::open(AgentFSOptions::with_id(&ctx.session_id)).await?;
agent.fs.write_file(path, content.as_bytes()).await?;
// ↑ 自动写入审计日志到 .agentfs/{session_id}.db
```

**优势：**
- 文件沙箱（CoW 隔离）— 误删/误写不污染原文件系统
- 自动审计 — 不再需要自建 `audit.jsonl`
- 单文件存储 — `.agentfs/{id}.db` 可复制/分享/快照

**代价：**
- 异步 API — 需 `tokio::Runtime::block_on` 桥接
- 新增依赖 — `agentfs` + `tokio` + `serde` + `serde_json`（大部分已有）
- DeepX 的部分 `std::fs` 操作（如 `file_search` 的 `grep`）不适合 AgentFS，仍需原生文件系统

**推荐策略：Phase 7.6 中 `agent.kv` 替代 `memory` 工具，`agent.tools` 替代 `audit.jsonl`，文件操作保留 `std::fs`（AgentFS 作为可选沙箱模式）。**

### async → sync 桥接

```rust
use std::sync::LazyLock;
use tokio::runtime::Runtime;

static RT: LazyLock<Runtime> = LazyLock::new(|| Runtime::new().unwrap());

// 打开 AgentFS（sync 封装）
pub fn open_sync(id: &str) -> Result<agentfs::AgentFS, anyhow::Error> {
    RT.block_on(AgentFS::open(AgentFSOptions::with_id(id)))
}
```

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
| `agentfs` | 7.6 | ❌ | ✅ deepx-tools |
| `turso` | 7.10 | ❌ | ✅ deepx-session (optional) |
| `tokio` | 7.6, 7.10 | 传递 | ✅ deepx-tools + deepx-session (optional) |
| `sha2` | 7.1, 7.9 | 传递 | ✅ deepx-tools |
| `hex` | 7.1 | 传递 | ⚠️ 可选（可用 `format!("{:x}")` 替代） |
| `windows` | 7.2 | ❌ | ✅ deepx-tools (cfg(windows)) |
| `pam` | 7.2 | ❌ | ✅ deepx-tools (cfg(not(windows))) |
