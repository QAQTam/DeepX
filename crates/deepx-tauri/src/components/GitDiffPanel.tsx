import { createSignal, For, Show, onMount, createEffect } from "solid-js";
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

interface GitDiffPanelProps {
  open: boolean;
  seed: string;
  onClose: () => void;
}

export default function GitDiffPanel(props: GitDiffPanelProps) {
  const { t } = useI18n();
  const [files, setFiles] = createSignal<GitFileEntry[]>([]);
  const [loading, setLoading] = createSignal(false);
  const [listError, setListError] = createSignal<string | null>(null);
  const [branch, setBranch] = createSignal("");
  const [branches, setBranches] = createSignal<{ name: string; current: boolean }[]>([]);
  const [switching, setSwitching] = createSignal(false);
  const [committing, setCommitting] = createSignal(false);
  const [commitMessage, setCommitMessage] = createSignal("");
  const [selectedFile, setSelectedFile] = createSignal<string | null>(null);
  const [diffLoading, setDiffLoading] = createSignal(false);
  const [diffError, setDiffError] = createSignal<string | null>(null);
  const [diffHtml, setDiffHtml] = createSignal<string | null>(null);
  const [showSwitchPrompt, setShowSwitchPrompt] = createSignal(false);
  const [pendingBranch, setPendingBranch] = createSignal("");

  // ── Load data when opened ──
  createEffect(() => {
    if (props.open && props.seed) {
      refresh();
      loadBranches();
    }
  });

  // ── Reset state when closed ──
  createEffect(() => {
    if (!props.open) {
      setFiles([]);
      setListError(null);
      setBranch("");
      setBranches([]);
      setSelectedFile(null);
      setDiffHtml(null);
      setDiffError(null);
      setCommitMessage("");
      setShowSwitchPrompt(false);
    }
  });

  async function refresh() {
    if (!props.seed) return;
    setLoading(true);
    setListError(null);
    try {
      const raw: GitFileEntry[] = JSON.parse(await invoke("cmd_get_git_diff", { seed: props.seed }));
      setFiles(raw);
      setDiffError(null);
    } catch (e) {
      console.error("git_diff error:", e);
      setFiles([]);
      setListError(String(e));
    }
    setLoading(false);
  }

  async function loadBranches() {
    try {
      const raw = await invoke<string>("cmd_list_branches", { seed: props.seed });
      const list: { name: string; current: boolean }[] = JSON.parse(raw);
      setBranches(list);
      const current = list.find((b) => b.current)?.name ?? "";
      setBranch(current);
    } catch (_) {
      setBranches([]);
    }
  }

  async function selectFile(path: string) {
    setSelectedFile(path);
    setDiffLoading(true);
    setDiffError(null);
    setDiffHtml(null);

    // Check if we already have the diff cached
    const cached = files().find((f) => f.path === path);
    if (cached?.diffHtml) {
      setDiffHtml(cached.diffHtml);
      setDiffLoading(false);
      return;
    }

    try {
      const rawDiff: string = await invoke("cmd_get_git_file_diff", {
        seed: props.seed,
        filePath: path,
      });
      const html =
        renderDiffHtml(rawDiff) ||
        '<div class="git-workspace-empty">No diff available</div>';
      // Cache the result
      setFiles((prev) =>
        prev.map((f) => (f.path === path ? { ...f, diffHtml: html } : f)),
      );
      setDiffHtml(html);
    } catch (e) {
      console.error("git_file_diff error:", e);
      setDiffError(String(e));
    }
    setDiffLoading(false);
  }

  async function switchBranch(name: string) {
    if (name === branch()) return;
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
      await invoke<string>("cmd_switch_branch", {
        seed: props.seed,
        branch: name,
        stash,
      });
      await refresh();
      await loadBranches();
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
      await invoke<string>("cmd_git_commit", {
        seed: props.seed,
        message: msg,
      });
      setCommitMessage("");
      setSelectedFile(null);
      setDiffHtml(null);
      await refresh();
    } catch (e) {
      console.error("commit:", e);
    }
    setCommitting(false);
  }

  const totalStats = () => {
    let a = 0,
      r = 0;
    for (const f of files()) {
      a += f.lines_added;
      r += f.lines_removed;
    }
    return { added: a, removed: r };
  };

  // ── Don't render when closed ──
  if (!props.open) return null;

  return (
    <div class="git-workspace-overlay" onClick={props.onClose}>
      <div class="git-workspace" onClick={(e) => e.stopPropagation()}>
        {/* ── Header ── */}
        <div class="git-workspace-header">
          <span class="git-workspace-title">{t().status.gitChanges}</span>

          <Show when={branches().length > 0} fallback={
            <span class="git-workspace-branch-select" style="cursor:default;">
              {branch() || "—"}
            </span>
          }>
            <select
              class="git-workspace-branch-select"
              value={branch()}
              onChange={(e) => switchBranch(e.currentTarget.value)}
              disabled={switching()}
            >
              <For each={branches()}>
                {(b) => (
                  <option value={b.name} selected={b.current}>
                    {b.name}
                  </option>
                )}
              </For>
            </select>
          </Show>

          <Show when={switching()}>
            <span class="git-spinner">⟳</span>
          </Show>

          <div class="git-workspace-stats">
            <Show when={files().length > 0}>
              <span>{files().length} {t().status.files}</span>
            </Show>
            <Show when={totalStats().added > 0}>
              <span class="git-workspace-stat-add">+{totalStats().added}</span>
            </Show>
            <Show when={totalStats().removed > 0}>
              <span class="git-workspace-stat-del">-{totalStats().removed}</span>
            </Show>
          </div>

          <div class="git-workspace-actions">
            <button
              class="git-workspace-icon-btn active"
              title="Unified diff"
              aria-label="Unified diff"
            >
              U
            </button>
            <button
              class="git-workspace-icon-btn"
              title="Split diff unavailable"
              aria-label="Split diff"
              disabled
            >
              S
            </button>
            <button
              class="git-workspace-icon-btn"
              onClick={refresh}
              disabled={loading()}
              title={t().skills.refresh}
            >
              {loading() ? <span class="git-spinner">⟳</span> : "↻"}
            </button>
            <button
              class="git-workspace-icon-btn"
              onClick={props.onClose}
              aria-label="Close"
            >
              ✕
            </button>
          </div>
        </div>

        {/* ── Branch switch prompt ── */}
        <Show when={showSwitchPrompt()}>
          <div class="git-switch-prompt">
            <span class="git-switch-prompt-msg">
              {t().status.switchPrompt.replace("{branch}", pendingBranch())}
            </span>
            <span class="git-switch-prompt-btns">
              <button class="git-switch-stash" onClick={() => doSwitch(pendingBranch(), true)}>
                {t().status.stashSwitch}
              </button>
              <button class="git-switch-cancel" onClick={() => { setShowSwitchPrompt(false); setPendingBranch(""); }}>
                {t().settings.cancel}
              </button>
            </span>
          </div>
        </Show>

        {/* ── Body ── */}
        <Show
          when={files().length > 0}
          fallback={
            <Show when={listError()} fallback={
              <div class="git-workspace-empty">
                {loading() ? "Loading..." : t().status.noChanges}
              </div>
            }>
              <div class="git-workspace-error" role="alert">
                <span>{listError()}</span>
                <button class="git-workspace-icon-btn" onClick={refresh}>Retry</button>
              </div>
            </Show>
          }
        >
          <div class="git-workspace-body">
            {/* Left: File list */}
            <div class="git-file-list">
              <For each={files()}>
                {(f) => (
                  <div
                    class={`git-file-item${selectedFile() === f.path ? " selected" : ""}`}
                    onClick={() => selectFile(f.path)}
                  >
                    <span
                      class="git-file-change-icon"
                      style={`color: ${CHANGE_COLORS[f.change] || "var(--text-muted)"}`}
                    >
                      {CHANGE_ICONS[f.change] || "?"}
                    </span>
                    <span class="git-file-path">{f.path}</span>
                    <span class="git-file-stats">
                      <Show when={f.lines_added > 0}>
                        <span class="git-file-stat-add">+{f.lines_added}</span>
                      </Show>
                      <Show when={f.lines_removed > 0}>
                        <span class="git-file-stat-del">-{f.lines_removed}</span>
                      </Show>
                    </span>
                  </div>
                )}
              </For>
            </div>

            {/* Right: Diff view */}
            <div class="git-diff-view">
              <Show
                when={selectedFile()}
                fallback={
                  <div class="git-diff-view-empty">
                    {t().status.noFileSelected ?? "Select a file to view diff"}
                  </div>
                }
              >
                <Show when={!diffLoading()} fallback={
                  <div class="git-diff-view-loading">Loading diff...</div>
                }>
                  <Show when={!diffError()} fallback={
                    <div class="git-diff-view-error">
                      <span>{diffError()}</span>
                      <button class="git-workspace-icon-btn" onClick={() => selectFile(selectedFile()!)}>
                        Retry
                      </button>
                    </div>
                  }>
                    <div class="git-diff-content" innerHTML={diffHtml() || ""} />
                  </Show>
                </Show>
              </Show>
            </div>
          </div>
        </Show>

        {/* ── Footer: Commit ── */}
        <Show when={files().length > 0}>
          <div class="git-workspace-footer">
            <input
              class="git-commit-input"
              type="text"
              value={commitMessage()}
              onInput={(e) => setCommitMessage(e.currentTarget.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  commit();
                }
              }}
              placeholder={t().status.commitPlaceholder}
              disabled={committing()}
            />
            <button
              class="git-commit-submit"
              onClick={commit}
              disabled={committing() || !commitMessage().trim()}
            >
              {committing() ? "..." : t().status.commit}
            </button>
          </div>
        </Show>
      </div>
    </div>
  );
}
