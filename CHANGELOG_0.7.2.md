# DeepX v0.7.2 更新日志 (2026-07-10)

## 工具侧核心改进

### 全部工具 JSON 结构化返回
- 所有工具统一返回 `{"timeis":"...","status":"ok|error","content":"..."}` JSON 格式
- 可通过 `status` 字段确定性判断工具执行成功/失败
- `handler!` 宏自动检测 JSON 或旧格式，双模式兼容
- 元数据与正文分离：行号坐标在 JSON 字段中，正文只保留纯代码

### file_read 读取去重缓存
- 全量读取自动计算内容哈希，再次读取同一文件时返回 `{"unchanged":true}` 省 token
- 连续两次读取同一文件自动放行原文（模型需要重新检查时）
- file_write/edit/delete 自动失效缓存
- LRU 淘汰，上限 64 条，Vec 线性扫描零依赖

### <file_state> 文件状态摘要注入
- 每轮 [Environment] 块自动注入当前工作区文件状态
- 显示路径、行数、最后操作类型（read/edited/created/deleted/moved）
- 按最近操作时间排序，上限 20 条
- 模型无需重复 file_read 确认文件状态

### exec_run argv 直接执行模式
- 新增 `argv` 参数：`exec_run({"argv":["cargo","check"]})` 直接 exec，绕开 PTY/shell
- `command` 字符串模式保留给需要管道的场景
- Windows 支持 pwsh/cmd/bash 三选一（`shell` 参数）
- 无 PTY 污染、无 ANSI 乱码、无编码问题

### System Prompt 五段重构
- 从 ROLE/PROTOCOL/RULES/SESSION 四段重组为 THINK_MAX/IDENTITY/TOOLS/PROTOCOL/RULES
- [IDENTITY] 首句对齐 Codex 定位风格
- [TOOLS] 段：工具组合策略 + 反模式（不教模型编码，只教工具用法）
- [PROTOCOL] + [RULES] 合并在同一文件：Shell 约定 + 响应格式 + 硬约束
- [UserMessage] 标记切分 [Environment] 元数据与用户真实输入

## 修复
- file_read 描述修正：从 "File operations: read,write,edit,search..." → "Read file contents with optional line range"
- file_edit required 字段修正：仅保留 [path]，支持 patterns 数组替代 old_string/new_string
- explore_scan 工具名引用修正：list_dir → file_list
- ToolKey(name,action) 二元组简化为 String 键，删除三层 fallback 查找逻辑
- git_tool 测试适配 JSON 返回格式

## 优化
- file_read 正文去除行号前缀（每行省 5 字符 × 200 行 = ~500 tokens/次）
- PROTOCOL 删除与 API schema 重复的工具速查表（省 ~150 tokens）
- file_state 追踪 + file_cache 缓存双模块分离

## 前端
- ToolRow 渲染升级：自动解析 JSON 结果，提取 diff 字段做语法高亮，content 字段做纯文本展示
- diffStats 适配 JSON 结果中的 diff 统计
- ChangelogModal 更新为 v0.7.2 条目

## 变更统计
- 55 files changed, +2931 lines, -1714 lines
- 核心 crates: deepx-tools, deepx-message, deepx-msglp, deepx-config, deepx-proto, deepx-tauri
