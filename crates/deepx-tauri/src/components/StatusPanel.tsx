import { For, Show, createSignal, onCleanup, onMount } from "solid-js";
import type { TaskInfo, ActivityEntry } from "../store/chat";
import { useI18n } from "../i18n";
import GitDiffPanel from "./GitDiffPanel";
import PlanReviewPanel from "./PlanReviewPanel";

const STATUS_ICON: Record<string, string> = {
  pending: "\u25cb", in_progress: "\u25cf", completed: "\u2713", cancelled: "\u2717",
};
const TOOL_ICONS: Record<string, string> = {
  read_file: "R", write_file: "W", edit_file: "E", edit_file_diff: "E",
  delete_file: "D", exec: ">", explore: "S", search: "Z", glob: "G",
  web_search: "@", web_fetch: "@", list_dir: "L", diff: "=",
  task_create: "T", task_update: "T", task_delete: "T", ask_user: "?",
};

type NavPage = null | "tasks" | "activity" | "plan" | "git";

export default function StatusPanel(props: {
  tasks: () => TaskInfo[];
  recentEdits: () => string[];
  activityLog: () => ActivityEntry[];
  seed: string;
  onTaskAction?: (action: "cancel" | "delete" | "ask", taskId: string, subject: string, description: string) => void;
}) {
  const { t } = useI18n();
  const [nav, setNav] = createSignal<NavPage>(null);
  const [nowTick, setNowTick] = createSignal(Date.now());
  const timer = setInterval(() => setNowTick(Date.now()), 1000);

  // Panel resize
  let panelW = Number(localStorage.getItem("deepx:panel-w")) || 340;
  const setPanelW = (w: number) => {
    panelW = w;
    document.documentElement.style.setProperty("--panel-w", w + "px");
    document.documentElement.style.setProperty("--panel-foot", w + "px");
    localStorage.setItem("deepx:panel-w", String(w));
  };
  onMount(() => setPanelW(panelW));
  onCleanup(() => clearInterval(timer));
  const elapsed = (ts: number) => {
    const _ = nowTick();
    const s = Math.floor((Date.now() - ts) / 1000);
    if (s < 60) return s + "s";
    if (s < 3600) return Math.floor(s / 60) + "m";
    return Math.floor(s / 3600) + "h";
  };

  const navTitle = () => {
    switch (nav()) {
      case "tasks": return t().status.tasks;
      case "activity": return t().status.activity;
      case "plan": return t().planReview?.title ?? "PLAN Review";
      case "git": return t().status.gitChanges;
      default: return t().status.title;
    }
  };

  return (
    <div class="status-panel">
      <div
        class="status-resize-handle"
        onMouseDown={(e) => {
          e.preventDefault();
          const startX = e.clientX;
          const startW = panelW;
          const handle = e.currentTarget as HTMLElement;
          handle.classList.add("active");
          const onMove = (ev: MouseEvent) => {
            const w = Math.max(240, Math.min(600, startW - (ev.clientX - startX)));
            setPanelW(w);
          };
          const onUp = () => {
            handle.classList.remove("active");
            document.removeEventListener("mousemove", onMove);
            document.removeEventListener("mouseup", onUp);
          };
          document.addEventListener("mousemove", onMove);
          document.addEventListener("mouseup", onUp);
        }}
      />
      <div class="status-panel-hd">
        <Show when={nav()} fallback={<span>{t().status.title}</span>}>
          <button class="status-nav-back" onClick={() => setNav(null)}>← {navTitle()}</button>
        </Show>
      </div>
      <div class="status-panel-body">

        {/* Level 1 — section tiles */}
        <Show when={!nav()}>
          {/* Tasks */}
          <div class="status-tile" onClick={() => setNav("tasks")}>
            <span class="status-tile-label">{t().status.tasks}</span>
            <Show when={props.tasks().length > 0}>
              <span class="status-tile-badge">{props.tasks().filter((t) => t.status === "completed").length}/{props.tasks().length}</span>
            </Show>
            <span class="status-tile-arrow">▶</span>
          </div>
          {/* Activity */}
          <div class="status-tile" onClick={() => setNav("activity")}>
            <span class="status-tile-label">{t().status.activity}</span>
            <Show when={props.activityLog().length > 0}>
              <span class="status-tile-badge">{props.activityLog().length}</span>
            </Show>
            <span class="status-tile-arrow">▶</span>
          </div>
          {/* PLAN Review */}
          <div class="status-tile" onClick={() => setNav("plan")}>
            <span class="status-tile-label">{t().planReview?.title ?? "PLAN Review"}</span>
            <span class="status-tile-arrow">▶</span>
          </div>
          {/* Git Changes */}
          <div class="status-tile" onClick={() => setNav("git")}>
            <span class="status-tile-label">{t().status.gitChanges}</span>
            <span class="status-tile-arrow">▶</span>
          </div>
        </Show>

        {/* Level 2 — Tasks full view */}
        <Show when={nav() === "tasks"}>
          <div class="status-level2-body">
          <Show when={props.tasks().length > 0} fallback={<div class="status-empty">{t().status.noTasks}</div>}>
            <For each={props.tasks()}>
              {(task) => (
                <div class={`status-row status-${task.status}${(task as any)._deleting ? ' deleting' : ''}`}>
                  <span class="status-row-icon">{STATUS_ICON[task.status] ?? "?"}</span>
                  <div class="status-row-info">
                    <span class="status-row-title">{task.id}: {task.subject}</span>
                    <span class="status-row-desc">{task.description}</span>
                  </div>
                  <Show when={props.onTaskAction}>
                    <div class="status-row-actions">
                      <Show when={task.status === "pending" || task.status === "in_progress"}>
                        <button class="task-btn task-btn-cancel" onClick={() => props.onTaskAction!("cancel", task.id, task.subject, task.description)} title="Cancel">✕</button>
                      </Show>
                      <Show when={task.status === "completed" || task.status === "cancelled"}>
                        <button class="task-btn task-btn-delete" onClick={() => props.onTaskAction!("delete", task.id, task.subject, task.description)} title="Delete">🗑</button>
                      </Show>
                      <button class="task-btn task-btn-ask" onClick={() => props.onTaskAction!("ask", task.id, task.subject, task.description)} title="Ask about this task">?</button>
                    </div>
                  </Show>
                </div>
              )}
            </For>
          </Show>
          </div>
        </Show>

        {/* Level 2 — Activity full view */}
        <Show when={nav() === "activity"}>
          <div class="status-level2-body">
          <Show when={props.activityLog().length > 0} fallback={<div class="status-empty">{t().status.noActivity}</div>}>
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
          </Show>
          </div>
        </Show>

        {/* Level 2 — PLAN Review full view */}
        <Show when={nav() === "plan"}>
          <div class="status-level2-body">
          <PlanReviewPanel seed={props.seed} />
          </div>
        </Show>

        {/* Level 2 — Git Changes full view */}
        <Show when={nav() === "git"}>
          <div class="status-level2-body">
          <GitDiffPanel seed={props.seed} />
          </div>
        </Show>

      </div>
    </div>
  );
}
