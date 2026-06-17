import { For, Show } from "solid-js";
import type { TaskInfo, ActivityEntry } from "../store/chat";
import { useI18n } from "../i18n";

const STATUS_ICON: Record<string, string> = {
  pending: "\u25cb", in_progress: "\u25cf", completed: "\u2713", cancelled: "\u2717",
};
const TOOL_ICONS: Record<string, string> = {
  read_file: "R", write_file: "W", edit_file: "E", edit_file_diff: "E",
  delete_file: "D", exec: ">", explore: "S", search: "Z", glob: "G",
  web_search: "@", web_fetch: "@", list_dir: "L", diff: "=",
  task_create: "T", task_update: "T", task_delete: "T", ask_user: "?",
};

export default function StatusPanel(props: {
  tasks: () => TaskInfo[];
  recentEdits: () => string[];
  activityLog: () => ActivityEntry[];
}) {
  const { t } = useI18n();
  const elapsed = (ts: number) => {
    const s = Math.floor((Date.now() - ts) / 1000);
    if (s < 60) return s + "s";
    if (s < 3600) return Math.floor(s / 60) + "m";
    return Math.floor(s / 3600) + "h";
  };

  return (
    <div class="status-panel">
      <div class="status-panel-hd">{t().status.title}</div>
      <div class="status-panel-body">

        {/* ── Tasks ── */}
        <div class="status-section">
          <div class="status-section-hd">
            {t().status.tasks}
            <Show when={props.tasks().length > 0}>
              <span class="status-section-badge">{props.tasks().filter((t) => t.status === "completed").length}/{props.tasks().length}</span>
            </Show>
          </div>
          <Show when={props.tasks().length > 0} fallback={<div class="status-empty">{t().status.noTasks}</div>}>
            <div class="status-section-body">
            <For each={props.tasks()}>
              {(task) => (
                <div class={`status-row status-${task.status}${(task as any)._deleting ? ' deleting' : ''}`}>
                  <span class="status-row-icon">{STATUS_ICON[task.status] ?? "?"}</span>
                  <div class="status-row-info">
                    <span class="status-row-title">{task.id}: {task.subject}</span>
                    <span class="status-row-desc">{task.description}</span>
                  </div>
                </div>
              )}
            </For>
            </div>
          </Show>
        </div>

        {/* ── {t().status.activity} ── */}
        <div class="status-section">
          <div class="status-section-hd">
            {t().status.activity}
            <Show when={props.activityLog().length > 0}>
              <span class="status-section-badge">{props.activityLog().length}</span>
            </Show>
          </div>
          <Show when={props.activityLog().length > 0} fallback={<div class="status-empty">{t().status.noActivity}</div>}>
            <div class="status-section-body">
            <For each={props.activityLog()}>
              {(entry) => (
                <div class={`status-row status-${entry.success ? "success" : "fail"}`}>
                  <span class="status-row-icon activity-icon">{TOOL_ICONS[entry.tool_name] ?? "*"}</span>
                  <div class="status-row-info">
                    <span class="status-row-title">{entry.tool_name}</span>
                    <span class="status-row-desc">{entry.summary}</span>
                  </div>
                  <span class="status-row-time">{elapsed(entry.time)}</span>
                </div>
              )}
            </For>
            </div>
          </Show>
        </div>

        {/* ── Files ── */}
        <div class="status-section">
          <div class="status-section-hd">
            {t().status.files}
            <Show when={props.recentEdits().length > 0}>
              <span class="status-section-badge">{props.recentEdits().length}</span>
            </Show>
          </div>
          <Show when={props.recentEdits().length > 0} fallback={<div class="status-empty">{t().status.noFiles}</div>}>
            <div class="status-section-body">
            <For each={props.recentEdits()}>
              {(edit) => {
                const [tool, ...pathParts] = edit.split(": ");
                const path = pathParts.join(": ");
                return (
                  <div class="status-row">
                    <span class="status-row-icon file-icon">{tool === "write_file" ? "W" : tool === "edit_file" ? "E" : "D"}</span>
                    <div class="status-row-info">
                      <span class="status-row-title">{tool}</span>
                      <span class="status-row-desc mono">{path}</span>
                    </div>
                  </div>
                );
              }}
            </For>
            </div>
          </Show>
        </div>

      </div>
    </div>
  );
}
