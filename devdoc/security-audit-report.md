# DeepX 安全审计报告 — 对照 P0 安全项

> 审计时间：2026-07-06
> 依据：政策文件 第三、四节 (#14-30)

## 1. guard.rs — 内容过滤

**现状**: `crates/deepx-gate/src/guard.rs` — 10 个硬编码关键词 + 3 个前缀白名单

**缺陷**:
- `academic:`/`research:`/`crypto:` 前缀**彻底绕过所有检查** — 即使内容包含"自杀教学"
- CJK 无词边界，零宽字符(U+200B)可绕过 NFKC
- 无语义理解，纯字符串匹配

**风险**: H | **建议**: 移除/限制 allowlist、加 CJK 分词边界、过滤零宽字符

---

## 2. safety.rs — 安全策略

**现状**: `crates/deepx-tools/src/safety.rs` — 仅 2 维度评估(risk × workspace)

**缺陷**:
- `RequireAuth` 在 `manager.rs:202-206` **被 TODO 注释跳过** — 所有需要确认的操作实际自动放行
- 无频率限制、无文件类型区分、无操作链检测

**风险**: M | **建议**: 扩展 ToolRisk 多维模型、实现 RequireAuth 弹窗、加频率/链式检测

---

## 3. auth.rs — 认证

**现状**: `crates/deepx-tools/src/auth.rs` — PIN + session_token

**缺陷**:
- PIN: 无长度下限、无盐、无重试锁定
- session_token: 熵源仅时间戳+PID，可预测，无过期/吊销
- GUI 模式下 `verify_pin` **无法工作**（无终端 stdin）

**风险**: H | **建议**: CSRNG salt、指数退避锁定、Tauri 原生确认对话框

---

## 4. audit.rs — 审计日志

**现状**: `crates/deepx-tools/src/audit.rs` — CSV 追加日志

**缺陷**:
- `args_hash` = 参数 SHA-256，**不记录原始参数**，无法审计"做了什么"
- `result` 仅 `"ok"/"fail"`，无具体输出
- 无数字签名/HMAC 链，日志可被任意修改

**风险**: M | **建议**: 记录可读参数摘要、HMAC 链防篡改、绑定用户/会话 ID

---

## 5. exec 模块 — 命令执行

**现状**: `crates/deepx-tools/src/exec.rs` — 通过 PTY 执行任意 shell

**缺陷**:
- **无沙箱** — 以父进程完整权限运行
- **无白名单** — `rm -rf /` 可直接执行
- **无注入防护** — 命令作为单一字符串传 shell
- auth 被禁用 → Destructive exec 自动放行

**风险**: H | **建议**: 命令白名单、Job Object/seccomp 沙箱、双重确认弹窗

---

## 6. 隐私 & 加密

**缺陷**:
- API key **明文**存储于 `config.json`
- 无隐私声明文档
- 无数据脱敏（密钥/密码在 exec 输出中明文泄露）
- 内存中敏感字符串无 zeroize

**风险**: H | **建议**: OS 密钥链/强加密存储、`secrecy::SecretString`、PRIVACY.md

---

## 总结

| 模块 | 风险 |
|---|---|
| guard.rs | H |
| safety.rs | M |
| auth.rs | H |
| audit.rs | M |
| exec.rs | H |
| 隐私/加密 | H |

**最紧急**: RequireAuth 被禁用 → 破坏性操作自动放行；exec 无沙箱；API key 明文存储。
