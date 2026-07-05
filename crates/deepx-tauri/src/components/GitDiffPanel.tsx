import { createSignal, For, Show } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { renderDiffHtml } from "../lib/diff";

export interface GitFileEntry {
  path: string;
  change: "added" | "deleted" | "modified" | "renamed";
  lines_added: number;
  lines_removed: number;
  diffHtml?: string;
}

const CHANGE_COLORS: Record<string, string> = {
  added: "var(--green)",
  modified: "var(--yellow)",
  deleted: "var(--red)",
  renamed: "var(--purple)",
};

const CHANGE_ICONS: Record<string, string> = {
  added: "+",
  modified: "~",
  deleted: "\u2212",
  renamed: "\u2192",
};

export default function GitDiffPanel(props: { seed: string }) {
  const [files, setFiles] = createSignal<GitFileEntry[]>([]);
  const [expanded, setExpanded] = createSignal<Set<string>>(new Set());
  const [loading, setLoading] = createSignal(false);

  async function refresh() {
    if (!props.seed) return;
    setLoading(true);
    try {
      const raw: GitFileEntry[] = JSON.parse(await invoke("cmd_get_git_diff", { seed: props.seed }));
      const prev = files();
      const prevMap = new Map(prev.map(f => [f.path, f]));
      const merged = raw.map(f => {
        const old = prevMap.get(f.path);
        if (old?.diffHtml) return { ...f, diffHtml: old.diffHtml };
        return f;
      });
      setFiles(merged);
    } catch (e) { console.error("git_diff error:", e); }
    setLoading(false);
  }

  function toggle(path: string) {
    setExpanded(prev => {
      const next = new Set(prev);
      if (next.has(path)) { next.delete(path); return next; }
      next.add(path);
      const f = files().find(x => x.path === path);
      if (f && !f.diffHtml && (f.change === "modified" || f.change === "added")) {
        loadDiff(path);
      }
      return next;
    });
  }

  async function loadDiff(path: string) {
    try {
      const rawDiff: string = await invoke("cmd_get_git_file_diff", { seed: props.seed, filePath: path });
      const html = renderDiffHtml(rawDiff);
      setFiles(prev => prev.map(f => f.path === path ? { ...f, diffHtml: html } : f));
    } catch (e) { console.error("git_file_diff error:", e); }
  }

  // Refresh on mount only; manual refresh via header click.
  // (No auto-polling — avoids Tauri IPC thread-pool starvation during streaming)
  refresh();

  const countByChange = () => {
    const c: Record<string, number> = {};
    for (const f of files()) { c[f.change] = (c[f.change] || 0) + 1; }
    return c;
  };

  return (
    <div class="git-diff-panel">
      <div class="git-diff-header" onClick={refresh}>
        <span class="git-diff-title">Git Changes</span>
        <Show when={loading()}><span class="git-diff-spinner">⟳</span></Show>
        <span class="git-diff-summary">
          <For each={Object.entries(countByChange())}>
            {([change, count]) => (
              <span class="git-diff-badge" style={`color: ${CHANGE_COLORS[change] || "var(--text-muted)"}`}>
                {CHANGE_ICONS[change] || "?"}{count}
              </span>
            )}
          </For>
        </span>
      </div>
      <Show when={files().length > 0} fallback={
        <div class="git-diff-empty">No changes</div>
      }>
        <div class="git-diff-list">
          <For each={files()}>
            {(file) => (
              <div class={`git-diff-card ${expanded().has(file.path) ? "expanded" : ""}`}>
                <div class="git-diff-card-hd" onClick={() => toggle(file.path)}>
                  <span class="git-diff-change-icon" style={`color: ${CHANGE_COLORS[file.change] || "var(--text-muted)"}`}>
                    {CHANGE_ICONS[file.change] || "?"}
                  </span>
                  <span class="git-diff-card-path">{file.path}</span>
                  <span class="git-diff-card-stats">
                    <Show when={file.lines_added > 0 || file.lines_removed > 0}>
                      <span class="git-diff-stat-add">+{file.lines_added}</span>
                      <span class="git-diff-stat-del">-{file.lines_removed}</span>
                    </Show>
                  </span>
                  <span class="git-diff-card-arrow">{expanded().has(file.path) ? "▼" : "▶"}</span>
                </div>
                <Show when={expanded().has(file.path)}>
                  <div class="git-diff-card-body">
                    <Show when={file.diffHtml} fallback={<div class="git-diff-loading">Loading...</div>}>
                      <div class="git-diff-content" innerHTML={file.diffHtml || ""} />
                    </Show>
                  </div>
                </Show>
              </div>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
}
