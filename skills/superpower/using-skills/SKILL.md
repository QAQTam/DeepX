---
name: using-skills
description: 任何对话开始时使用——确定如何发现和使用 skills。在任何响应（包括澄清问题）之前必须检查是否有适用的 skill。
---

# Using Skills

## 核心规则

**在做出任何响应或操作之前，先检查是否有适用的 skill。** 包括澄清问题、探索代码库、查看文件。即使只有 1% 的可能性，也必须检查。

如果你认为某个 skill 可能适用——你没有选择，必须使用它。这不是可协商的。

## 如何检查

1. 查看本 prompt 底部的 `[SKILLS]` 段（catalog），里面列出了所有可用的 skill（名称 + 描述 + 文件路径）
2. 如果任务匹配某个 skill 的描述，用 `read` 工具加载对应的 `SKILL.md` 文件
3. 读完 SKILL.md 正文后再行动
4. 如果 skill 有 checklist 或步骤表，逐项执行

## Skill 优先级

多个 skill 适用时，流程类 skill 优先——它们确定方法论，然后才是实现类 skill：

- "帮我重构这个模块" → 先查看 `deepx-refactor-workflow`，再查架构相关 skill
- "这个 crash 怎么回事" → 先查看 `deepx-debug`（或 `systematic-debugging`），再查具体领域 skill
- "我要新做一个功能" → 先查看 `brainstorming`，设计方案，再查 `writing-plans` 或 `test-driven-development`

## 红旗信号

以下想法意味着 STOP——你在找借口跳过 skill：

| 想法 | 现实 |
|------|------|
| "这只是个简单问题" | 问题就是任务。检查 skill。 |
| "我先了解一下代码" | Skill 告诉你**怎么**了解。先检查。 |
| "我先看看 git/文件" | 文件缺少对话上下文。先检查 skill。 |
| "不需要这么正式" | 简单的变复杂。用 skill。 |
| "我记得这个 skill" | Skill 会更新。读当前版本。 |
| "我先做这一件小事" | 做事之前先检查。 |

## 注意事项

- DeepX 使用 `read` 工具加载 SKILL.md 文件（不是 `Skill` 工具）
- Skill 中的工具名可能不同（如 Superpowers 的 `write_file` 对应 DeepX 的 `write`），但概念相同
- 用户指令 > skill 指令 > 默认行为
