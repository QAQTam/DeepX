# DeepX 开发规范与架构决策记录

> 本目录记录了 DeepX 项目的开发规范、架构决策、以及从重大事故中总结的经验。

## 文档列表

| 文档 | 说明 |
|------|------|
| [`development-standards.md`](development-standards.md) | 开发规范、代码审查清单、提交规范 |
| [`disaster-recovery-report.md`](disaster-recovery-report.md) | 2026-06-30 崩溃灾难完整复盘报告 |
| [`windows-com-rules.md`](windows-com-rules.md) | Windows COM/WinRT 编程规则（本次事故核心教训） |

## 核心原则

1. **第三方库不保证平台安全** — 特别是跨平台库的 Windows 后端
2. **Windows COM 必须显式管理** — 不可依赖隐式回退路径
3. **线程即责任** — spawn 一个线程就要管理它的完整生命周期
4. **崩溃地址不变不等于根因不变** — 区分触发条件与底层原因
