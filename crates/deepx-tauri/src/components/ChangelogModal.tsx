import { For } from "solid-js";

interface ChangelogEntry {
  tag: "feature" | "fix" | "perf" | "remove";
  title: string;
}

const CHANGELOG: ChangelogEntry[] = [
  // ── v0.8.0 ──
  { tag: "feature", title: "工具集精简重组：file_edit 合并三模式（精确匹配 / 模糊匹配 / 行号定位），支持跨文件批量编辑；file_read 支持多文件批量读取" },
  { tag: "feature", title: "web 工具合并：web_fetch + web_search 统一为 web，URL 抓取与 Bing RSS 搜索自动分流" },
  { tag: "feature", title: "context7 独立工具：搜库解析 / 文档查询自动分流，修正为官方 v2 API（/libs/search + /context）" },
  { tag: "feature", title: "exec_run 真实超时控制：try_wait 轮询 + 超时自动 kill 子进程，返回 timed_out 标志" },
  { tag: "feature", title: "exec 输出按 token 精确截断：count_tokens + 头尾保留策略，替代 char × 4 估算" },
  { tag: "feature", title: "exec 输出 OOM 防护：spawn + 流式管道读取，硬上限 5MB，超额自动排空" },
  { tag: "feature", title: "web_search 后端起用 Bing RSS（cn.bing.com/search?format=rss），零 API key 配置，返回结构化 JSON" },
  { tag: "remove", title: "删除 PTY 子系统：exec 不再支持 shell 模式（command 参数），统一使用 argv 数组" },
  { tag: "remove", title: "删除余额查询：gate 层 query_balance 及 StreamEvent::Balance 移除" },
  { tag: "remove", title: "删除 Content delta 批量缓冲：后端不再积攒 10ms/20char 再发送，直接逐帧推送" },
  { tag: "remove", title: "删除 deepx-sed 整个 crate（已从 workspace 移除）" },
  { tag: "remove", title: "删除全部旧 prompt 文件（think_max/role/protocol/rules/session），替换为精简版 backend_prompt.md" },
  { tag: "fix", title: "exec_run 退出码处理修正：非零退出码不再标记为 error，返回 status:completed 由模型自行判断" },
  { tag: "fix", title: "file_edit_diff 错误返回格式统一为 [ERROR] 前缀纯文本" },
  { tag: "fix", title: "resolve_workspace_path 路径规范化优化" },
  { tag: "fix", title: "前端 ToolCard 更新：适配合并后的工具名、新增 web/context7 映射、exec 显示支持 argv 数组" },
];

const TAG_ICONS: Record<string, string> = {
  feature: "✨",
  fix: "🐛",
  perf: "⚡",
  remove: "🗑️",
};

const TAG_LABELS: Record<string, string> = {
  feature: "新增",
  fix: "修复",
  perf: "优化",
  remove: "移除",
};

export default function ChangelogModal(props: { onClose: () => void }) {
  return (
    <div class="changelog-overlay" onClick={props.onClose}>
      <div class="changelog-dialog" onClick={(e) => e.stopPropagation()}>
        <div class="changelog-header">
          <span class="changelog-title">DeepX 更新日志</span>
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
