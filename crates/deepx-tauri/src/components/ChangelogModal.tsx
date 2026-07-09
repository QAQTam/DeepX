import { For } from "solid-js";

interface ChangelogEntry {
  tag: "feature" | "fix" | "perf";
  title: string;
}

const CHANGELOG: ChangelogEntry[] = [
  { tag: "feature", title: "SQLite 消息存储（预览功能）：per-session 数据库、双写启用、设置页开关（实时生效，无需重启）、读路径优先走库回退 JSONL、绿点指示器" },
  { tag: "feature", title: "JSONL → SQLite 迁移工具：待迁移计数、双弹窗确认、10 秒进度条动画、INSERT OR REPLACE 去重、一键迁移全部历史会话" },
  { tag: "feature", title: "Activity 面板改造：后端 Turso 查询接口、工具执行记录格式化（时间 + 文件名/命令 + 状态）、刷新不丢失" },
  { tag: "feature", title: "工具卡片重构：ToolRow 替换 ToolCallCard 胶囊体、i18n 中文化（编辑/写入/执行）、内联单行展开设计、MiSans 字体、12px 对齐" },
  { tag: "feature", title: "exec 流式输出优化：PTY 定时 + 定长双重 flush（50ms / 512B）、保留空行、ANSI 颜色支持、ExecOutput JSON 自动解析" },
  { tag: "fix", title: "System prompt 重复回传修复：from_messages 去重、build_context_for_gate 删除双来源注入分支、push_system 增加幂等性检查" },
  { tag: "fix", title: "截断层修复：truncate_tool_result 从存储层移到对话上下文层，工具结果存储完整版，LLM 按需折叠截断" },
  { tag: "fix", title: "file_edit/write 编辑完成后始终保留完整 diff body（store 存完整、LLM 看折叠、用户展开看 diff）" },
  { tag: "fix", title: "Turso tokio runtime 嵌套 panic 修复、PRAGMA execute_batch 行返回值修复、WAL checkpoint 补齐" },
  { tag: "fix", title: "数据库开关变更后热重载 agents（AtomicBool + ReloadConfig），无需重启程序" },
  { tag: "fix", title: "前端编辑/执行工具右侧始终显示文件名或命令（args_json→args_display 字段修正）" },
  { tag: "perf", title: "save_append Turso 写入改为批量 insert_messages_batch，去掉逐条 insert_message 循环" },
  { tag: "perf", title: "config.toml 旧值覆盖修复：PersistentDatabaseConfig.enabled 改为 Option<bool>" },
];

const TAG_ICONS: Record<string, string> = {
  feature: "✨",
  fix: "🐛",
  perf: "⚡",
};

const TAG_LABELS: Record<string, string> = {
  feature: "新增",
  fix: "修复",
  perf: "优化",
};

export default function ChangelogModal(props: { onClose: () => void }) {
  return (
    <div class="changelog-overlay" onClick={props.onClose}>
      <div class="changelog-dialog" onClick={(e) => e.stopPropagation()}>
        <div class="changelog-header">
          <span class="changelog-title">v0.7.1 更新日志</span>
          <button class="changelog-close" onClick={props.onClose}>✕</button>
        </div>
        <div class="changelog-body">
          <For each={CHANGELOG}>
            {(entry) => (
              <div class={`changelog-entry changelog-${entry.tag}`}>
                <span class="changelog-tag">{TAG_ICONS[entry.tag]} {TAG_LABELS[entry.tag]}</span>
                <span class="changelog-text">{entry.title}</span>
              </div>
            )}
          </For>
        </div>
      </div>
    </div>
  );
}
