import { For } from "solid-js";

interface ChangelogEntry {
  tag: "feature" | "fix" | "perf";
  title: string;
}

const CHANGELOG: ChangelogEntry[] = [
  { tag: "feature", title: "全部工具返回结构化 JSON：file_read/file_write/file_edit/exec/git/web/task/plan/memory/process 统一 JSON 格式，带 timeis/status/content" },
  { tag: "feature", title: "文件读取缓存去重：file_read 返回 content hash，未变更文件再读直接返回 'unchanged'，连续两次读取自动放行原文" },
  { tag: "feature", title: "文件状态摘要注入：每轮 [Environment] 注入 <file_state> 块，显示最近 20 个文件路径/行数/操作类型，模型无需重复读取" },
  { tag: "feature", title: "exec_run 支持 argv 直接执行模式（对标 Codex），绕开 PTY/shell 层，无管道污染，更快更稳；command 字符串模式保留给管道场景" },
  { tag: "feature", title: "exec_run 支持模型主动选择 shell：pwsh/cmd/bash（Windows），bash/zsh/sh（Unix）" },
  { tag: "feature", title: "System prompt 重构：THINK_MAX/IDENTITY/TOOLS/PROTOCOL/RULES 五段，融合 Codex 教学式风格，[UserMessage] 标记切分元数据与用户输入" },
  { tag: "fix", title: "file_read 描述修正：从 'File operations: read,write,edit,search...' 改为 'Read file contents with optional line range'" },
  { tag: "fix", title: "file_edit required 字段修正：从 [path,old_string,new_string] 改为仅 [path]，接受 patterns 数组或 old_string/new_string 组合" },
  { tag: "fix", title: "explore_scan 描述修正：引用的 'list_dir' 改为正确工具名 'file_list'" },
  { tag: "fix", title: "ToolKey 二元组简化为 String 键，删除三层 fallback 查找逻辑（--30行）" },
  { tag: "perf", title: "file_read 正文去除行号前缀（每行省 5 字符），行号移至 JSON 元数据 start_line/end_line" },
  { tag: "perf", title: "[PROTOCOL] 段删除冗余工具速查表（与 API schema 重复），省 ~150 tokens" },
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
          <span class="changelog-title">v0.7.2 更新日志</span>
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
