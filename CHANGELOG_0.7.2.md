# DeepX v0.7.1 → v0.7.2 改动清单

## 功能更新

| 功能 | 模块 | 文件 | 预期表现 |
|---|---|---|---|
| **Git 分支切换** | 前端 + 后端 | `agent_bridge.rs:1083-1157`, `lib.rs:38-39`, `GitDiffPanel.tsx:93-136`, `git-diff-panel.css:22-29` | 下拉框选中分支立即 force checkout，diff 自动刷新 |
| **Git Commit 按钮** | 前端 + 后端 | `agent_bridge.rs:1159-1195`, `lib.rs:40`, `GitDiffPanel.tsx:176-197`, `git-diff-panel.css:69-93` | 有变更时显示 [提交] 按钮，点击展开输入框，Enter/✓ 提交，自动刷新 diff |
| **切分支脏区提示** | 前端 + 后端 | `agent_bridge.rs:1111-1157`, `GitDiffPanel.tsx:200-218`, `git-diff-panel.css:95-114` | 有未提交修改时切分支，弹出「暂存并切换 / 丢弃并切换 / 取消」三选一 |
| **Agent Git 工具 ×4** | deepx-tools | `git_tool.rs:425-720`（exec_branch/checkout/merge/restore） | Agent 可创建/切换/合并分支、恢复文件，覆盖全离线单人工作流 |
| **删除终端 TUI** | 工作区 | 删除 `crates/deepx-terminalui/`、`crates/deepx-terminal/`，`Cargo.toml` members 清理 | 工作区成员从 17 → 15 个 crate，零外部引用，全仓 check 通过 |

## 质量更新

| 类别 | 模块 | 文件 | 预期表现 |
|---|---|---|---|
| **I18n 国际化** | 前端 | `i18n/en.ts:146-152`, `i18n/zh.ts:148-154`, `GitDiffPanel.tsx:180,186,204,208,211,214` | 所有 Git UI 文案从硬编码移入 i18n，中英文自动切换 |
| **Tool Call 误触发修复** | deepx-msglp / deepx-gate | `util.rs:52-54`, `tool_parser.rs:3-7` | 聊天中裸写 `<invoke>` 或 DSML 示例不再被当作真实工具调用执行 |
| **Tool Schema 描述更新** | deepx-tools | `git_tool.rs:757-775,806-812` | git/log、git/diff、git/commit 描述不再写 "no branch/merge"，改为指向新工具 |
| **流式渲染性能** | 前后端全链路 | `agent_bridge.rs:367-371`（去 log/双发/clone），`lib.rs:302`（flush 10→2ms），`chat.ts`（去所有 RAF 合批） | 端到端延迟 ~30ms → ~2ms，打字机效果无感知滞后 |
| **Commit 按钮样式** | 前端 CSS | `git-diff-panel.css:70-71,112-113` | 从亮色 `var(--bg-accent)` 改为深黑 `#1e1e1e` |
