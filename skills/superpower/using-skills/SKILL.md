---
name: using-skills
description: Use when starting any conversation, before responding, asking questions, or taking actions that may have an applicable skill.
---

# Using Skills

## 核心规则

**在做出任何响应或操作之前，先检查是否有适用的 skill。** 包括澄清问题、探索代码库、查看文件。即使只有 1% 的可能性，也必须检查。

如果你认为某个 skill 可能适用——你没有选择，必须使用它。这不是可协商的。

## 如何检查

1. 查看 system prompt 的稳定 skills catalog，其中按真实名称列出 name 和 description
2. 如果任务匹配某个 skill 的描述，使用 **`skills(action=activate, name="skill-name")`** 工具加载
3. **绝对禁止**使用 `read` 工具直接读取 `SKILL.md` 文件——只允许 `skills` 工具的加载命令
4. Skill 激活后其指令会注入当前上下文，严格遵循其中的工作流和方法论
5. 如果 skill 有 checklist 或步骤表，逐项执行

## Skill 优先级

多个 skill 适用时，流程类 skill 优先——它们确定方法论，然后才是实现类 skill：

- "帮我重构这个模块" → 先激活 `deepx-refactor-workflow`，再查架构相关 skill
- "这个 crash 怎么回事" → 先激活 `deepx-debug`（或 `systematic-debugging`），再查具体领域 skill
- "我要新做一个功能" → 先激活 `brainstorming`，设计方案，再查 `writing-plans` 或 `test-driven-development`

## 红旗信号

以下想法意味着 STOP——你在找借口跳过 skill：

| 想法 | 现实 |
|------|------|
| "这只是个简单问题" | 问题就是任务。检查 skill。 |
| "我先了解一下代码" | Skill 告诉你**怎么**了解。先检查。 |
| "我先看看 git/文件" | 文件缺少对话上下文。先检查 skill。 |
| "不需要这么正式" | 简单的变复杂。用 skill。 |
| "我记得这个 skill" | Skill 会更新。激活当前版本。 |
| "我先做这一件小事" | 做事之前先检查。 |

## 注意事项

- Skill 通过 `skills(action=activate)` 动态加载，**不要**用 `read` 直读 `SKILL.md`
- Skill 中提到的工具名可能不同（如 Superpowers 的 `write_file` 对应 DeepX 的 `write`），但概念相同
- 用户指令 > skill 指令 > 默认行为
- 当 skill 的指导与用户显式指令冲突时，先指出冲突再让用户决定
