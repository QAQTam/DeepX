# dsx-tui Bug Report

审查日期: 2025-xx-xx · 范围: `crates/dsx-tui`

---

## 🔴 Bug 1：Session 恢复时工具参数丢失

**文件**: `crates/dsx-tui/src/app.rs:841`

```rust
// BUG: input.as_str() 对 JSON 对象返回 None
let args: String = input.as_str().unwrap_or("").into();
```

**原因**: `ContentBlock::ToolUse` 的 `input` 字段是 `serde_json::Value`，工具调用参数是 JSON 对象（如 `{"path":"src/main.rs","pattern":"foo"}`），不是 JSON 字符串。`as_str()` 对对象返回 `None`，导致 `args` 恒为空。

**后果**: 恢复旧会话时，工具调用条只显示 `"reading: "` 而不带文件名/路径。

**修复**: 将 `input.as_str()` 替换为 `input.to_string()` 或 `serde_json::to_string(&input).unwrap_or_default()`。

---

## 🔴 Bug 2：ContentDelta 强制滚动复位

**文件**: `crates/dsx-tui/src/app.rs:673`

```rust
Agent2Ui::ContentDelta { delta, reasoning } => {
    self.debug.streaming = true;
    self.scroll_offset = 0;  // ← 每次 delta 都弹回底部
```

**原因**: 每次流式 delta 到达，滚动偏移无条件重置为 0。

**后果**: 用户在 streaming 期间向上滚动查看历史内容，会被持续弹回底部。

**修复**: 仅在 `self.scroll_offset == 0`（已在最新位置）时保持置底，用户主动滚动后不再复位。

---

## 🟡 Bug 3：`append_last` 全量重渲染 O(n²)

**文件**: `crates/dsx-tui/src/app.rs:654-658`

```rust
fn append_last(&mut self, content: &str) {
    if let Some(last) = self.messages.last_mut() {
        last.content.push_str(content);
        last.lines = crate::markdown::render_content(&last.content); // 全量重渲染
    }
}
```

**原因**: 每次收到一小段 delta（可能仅几个 token），就把整个消息重新跑一遍 markdown 解析渲染。

**后果**: 长对话中、尤其是包含大代码块或表格时，性能随消息长度线性退化 → O(n²)。

**修复**: 增量追加渲染行，而非每次都从零开始。或仅在新行边界（`\n`）触发重渲染。

---

## 🟡 Bug 4：UTF-8 字节截断边界

**文件**: `crates/dsx-tui/src/app.rs:643`

```rust
const MAX_STORED: usize = 50_000;
// 这里 .len() 返回字节数，不是字符数
let content = if content.len() > MAX_STORED {
```

**原因**: `String::len()` 计算的是 UTF-8 字节长度。中文字符 1 个 = 3 字节。

**后果**: 对于纯中文内容，实际存了约 16,666 个字符就被截断，远低于预期的 50,000。

**修复**: 使用 `.chars().count()` 或按字符边界截断。

---

## 🟡 Bug 5：未处理的 Agent2Ui 变体

**文件**: `crates/dsx-tui/src/app.rs:799`

`handle_frame` 的 match 用 `_ => {}` 静默忽略了以下变体：

| 变体 | 丢失功能 |
|------|---------|
| `ToolProgress` | 工具执行中的流式输出（如实时命令输出） |
| `ToolState` | 当前 turn 中已探索/读取/写入的文件列表 |
| `CachePrediction` | 模型返回的预测缓存命中率 |
| `ShutdownAck` | Agent 关闭确认信号 |

---

## ⚪ 其他小问题

### 冗余代码 (main.rs:375)
```rust
let text = app.input.drain(..).collect::<String>();  // drain 已清空 input
app.input.clear();  // ← 冗余
```

### 代码块制表符替换 (markdown.rs:270)
```rust
let l = line.replace('\t', "    ");
```
仅替换了 `\t`，但其他控制字符或 ANSI 序列会原样传给 ratatui，可能导致渲染异常。

### 超长单词换行计数 (ui.rs count_wrap_rows)
`count_wrap_rows` 按单词（word）拆分，未处理超长单词（如 URL）的内部换行。可能导致滚动位置略微偏差。

---

## 优先级建议

| 优先级 | Bug | 影响 |
|--------|-----|------|
| P0 | Bug 1 | 功能缺陷 — 会话恢复时工具参数丢失 |
| P0 | Bug 2 | 交互缺陷 — 流式输出时无法阅读历史 |
| P1 | Bug 3 | 性能退化 — 长对话卡顿 |
| P1 | Bug 4 | 边界错误 — 中文截断过早 |
| P2 | Bug 5 | 功能缺失 — 丢失部分 UI 反馈 |
