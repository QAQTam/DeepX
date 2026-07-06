
- [✓] ] P1: chat.ts：添加计时与t/s数据流 — 在 chat.ts 的 handleTurnStart 中记录 turn_started_at 时间戳，handleRoundDelta 首次 thinking/answer 时记录对应时间戳，handleTurnEnd 时计算 t/s 并存入 Turn。Turn.usage 扩展 completion_tokens 字段。新增 Turn.metrics: { started_at, first_answer_at, thinking_ms, output_ms, tokens_per_sec }。。Deps: none。Effort: 2h。

- [✓] ] P2: ThinkingBlock：实时思考计时器 + 沉浸式风格 — 重写 ThinkingBlock 组件：默认折叠显示"正在思考… X.Xs"带实时秒表；展开后停止计时显示"思考完成 (X.Xs)"；内容区无背景框。计时器从 props 传入（或组件内自维护）。去掉 border-left 和 background 卡片框。。Deps: none。Effort: 2h。

- [✓] ] P3: ToolCallCard：重写为内联状态行 + 中文动词映射 — 重写 ToolCallCard 为内联状态行，去掉卡片框（border-left/background/border-radius）。运行中显示: 图标 + 中文动词（探索/读取/写入/搜索/执行…）+ 路径/参数摘要 + 实时计时器 + 脉冲动画。完成后显示: ✅/❌ + 耗时。点击展开内联输出。添加完整的中文动词映射表。toolIcon 扩展为 {icon, verb} 结构。。Deps: none。Effort: 3h。

- [✓] ] P4: MessageItem：t/s 展示 + 沉浸式布局简化 — MessageItem 增加 t/s 信息展示：在最后一个 round 的 answer 底部，当 turn.status==='complete' 且 metrics.tokens_per_sec 存在时，渲染"42 t/s · 总计 256 tokens"行。同时简化布局：去掉 bubble-user/bubble-ai 背景/圆角，msg-avatar 缩小变纯文字，msg-round 去掉 border-left。。Deps: none。Effort: 2h。

- [✓] ] P5: message-list.css + tool-call-card.css：沉浸式重写 — 重写 message-list.css：去掉 .bubble-user/.bubble-ai 的 background + border-radius（改为纯文本或极淡背景）。去掉 .tool-card 所有卡片框样式（替换为内联状态行样式）。去掉 .think-block 的 border-left + background。去掉 .msg-round 的 border-left。弱化 .msg-avatar（缩小、去背景色）。新增 .inline-status、.think-timer、.tps-footer 等样式。。Deps: none。Effort: 2h。

- [✓] ] P6: i18n：工具状态中文动词 + t/s 格式 — i18n 新增翻译键：tool.status 对象包含 explore/reading/writing/editing/deleting/moving/copying/diffing/listing/searching/exec/web/webFetch/git/task/plan/ask/memory/process 的中文动词（如"正在探索""正在读取""正在写入"），以及 tokenSpeed 格式串"{n} t/s · 总计 {total} tokens"。zh.ts 和 en.ts 同步添加。。Deps: none。Effort: 1h。 | i18n strings needed by P2/P3/P4

- [✓] ] P7: 三主题适配验证 — 验证三种主题（light/dark/dark-gray）下沉浸式新样式的一致性。确保去框后的文本可读性、内联工具行的对比度、思考计时器的可见性、t/s 信息在暗色主题下的颜色。必要时添加 CSS 变量（如 --inline-status-bg、--think-muted 等）。。Deps: none。Effort: 1h。
