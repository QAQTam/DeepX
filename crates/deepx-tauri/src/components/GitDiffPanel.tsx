import { createSignal, For, Show, onMount } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "../i18n";
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
  const { t } = useI18n();
  const [files, setFiles] = createSignal<GitFileEntry[]>([]);
  const [expanded, setExpanded] = createSignal<Set<string>>(new Set());
  const [loading, setLoading] = createSignal(false);
  const [branch, setBranch] = createSignal("");
  const [branches, setBranches] = createSignal<{name: string; current: boolean}[]>([]);
  const [switching, setSwitching] = createSignal(false);
  const [committing, setCommitting] = createSignal(false);
  const [showCommitInput, setShowCommitInput] = createSignal(false);
  const [commitMessage, setCommitMessage] = createSignal("");
  const [showSwitchPrompt, setShowSwitchPrompt] = createSignal(false);
  const [pendingBranch, setPendingBranch] = createSignal("");

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
      const html = renderDiffHtml(rawDiff) || "<div class='status-empty'>No diff available (new/untracked file)</div>";
      setFiles(prev => prev.map(f => f.path === path ? { ...f, diffHtml: html } : f));
    } catch (e) {
      console.error("git_file_diff error:", e);
      setFiles(prev => prev.map(f => f.path === path ? { ...f, diffHtml: "<div class='status-empty'>Diff failed</div>" } : f));
    }
  }

  // Refresh on mount + load branches
  onMount(() => {
    refresh();
    loadBranches();
  });

  async function loadBranches() {
    try {
      const raw = await invoke<string>("cmd_list_branches", { seed: props.seed });
      setBranches(JSON.parse(raw));
      invoke<string>("cmd_get_git_branch", { seed: props.seed })
        .then(setBranch).catch(() => setBranch(""));
    } catch (_) { setBranches([]); }
  }

  async function switchBranch(name: string) {
    if (name === branch()) return;
    // If there are uncommitted changes, ask the user
    if (files().length > 0) {
      setPendingBranch(name);
      setShowSwitchPrompt(true);
      return;
    }
    await doSwitch(name);
  }

  async function doSwitch(name: string, stash: boolean = false) {
    setSwitching(true);
    setShowSwitchPrompt(false);
    try {
      const newHead = await invoke<string>("cmd_switch_branch", { seed: props.seed, branch: name, stash });
      setBranch(newHead);
      setFiles([]); setExpanded(new Set<string>());
      await refresh();
    } catch (e) {
      console.error("switch branch:", e);
    }
    setSwitching(false);
  }

  async function commit() {
    const msg = commitMessage().trim();
    if (!msg) return;
    setCommitting(true);
    try {
      await invoke<string>("cmd_git_commit", { seed: props.seed, message: msg });
      setCommitMessage("");
      setShowCommitInput(false);
      await refresh();
    } catch (e) {
      console.error("commit:", e);
    }
    setCommitting(false);
  }

  const totalStats = () => {
    let a = 0, r = 0;
    for (const f of files()) { a += f.lines_added; r += f.lines_removed; }
    return { added: a, removed: r };
  };

  const countByChange = () => {
    const c: Record<string, number> = {};
    for (const f of files()) { c[f.change] = (c[f.change] || 0) + 1; }
    return c;
  };

  return (
    <div class="git-diff-panel">
      <div class="git-diff-header">
        <Show when={branches().length > 0} fallback={
          <span class="git-diff-title" onClick={refresh}>{branch() || t().status.gitChanges}</span>
        }>
          <select
            class="git-branch-select"
            value={branch()}
            onChange={(e) => switchBranch(e.currentTarget.value)}
            disabled={switching()}
            onClick={(e) => e.stopPropagation()}
          >
            <For each={branches()}>
              {(b) => <option value={b.name} selected={b.current}>{b.name}</option>}
            </For>
          </select>
        </Show>
        <Show when={switching()}><span class="git-diff-spinner">⟳</span></Show>
        <Show when={loading()}><span class="git-diff-spinner" onClick={refresh}>⟳</span></Show>
        <span class="git-diff-summary">
          <Show when={totalStats().added > 0 || totalStats().removed > 0}>
            <span class="git-diff-stat-add">+{totalStats().added}</span>
            <span class="git-diff-stat-del">-{totalStats().removed}</span>
          </Show>
        </span>
        {/* Commit button / inline input */}
        <Show when={files().length > 0}>
          <Show when={showCommitInput()} fallback={
            <button class="git-commit-btn" onClick={() => setShowCommitInput(true)}
              disabled={committing() || switching()}>{t().status.commit}</button>
          }>
            <span class="git-commit-inline">
              <input
                type="text"
                class="git-commit-input"
                placeholder={t().status.commitPlaceholder}
                value={commitMessage()}
                onInput={(e) => setCommitMessage(e.currentTarget.value)}
                onKeyDown={(e) => { if (e.key === "Enter") commit(); if (e.key === "Escape") { setShowCommitInput(false); setCommitMessage(""); }}}
                disabled={committing()}
                ref={(el) => { if (el) setTimeout(() => el.focus(), 0); }}
              />
              <button class="git-commit-ok" onClick={commit} disabled={committing() || !commitMessage().trim()}>✓</button>
              <button class="git-commit-cancel" onClick={() => { setShowCommitInput(false); setCommitMessage(""); }}>✗</button>
            </span>
          </Show>
        </Show>
      </div>

      {/* Switch-branch prompt when there are uncommitted changes */}
      <Show when={showSwitchPrompt()}>
        <div class="git-switch-prompt">
          <span class="git-switch-prompt-msg">
            {t().status.switchPrompt.replace("{branch}", pendingBranch())}
          </span>
          <span class="git-switch-prompt-btns">
            <button class="git-switch-stash" onClick={() => doSwitch(pendingBranch(), true)}>
              {t().status.stashSwitch}
            </button>
            <button class="git-switch-discard" onClick={() => doSwitch(pendingBranch(), false)}>
              {t().status.discardSwitch}
            </button>
            <button class="git-switch-cancel" onClick={() => { setShowSwitchPrompt(false); setPendingBranch(""); }}>
              {t().settings.cancel}
            </button>
          </span>
        </div>
      </Show>
      <Show when={files().length > 0} fallback={
        <div class="git-diff-empty">{t().status.noChanges}</div>
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