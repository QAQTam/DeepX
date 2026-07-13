import { For, Show, createSignal, createMemo, createEffect, onCleanup, onMount } from "solid-js";
import type { TaskInfo, ActivityEntry, SkillInfo } from "../store/chat";
import { useI18n } from "../i18n";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import GitDiffPanel from "./GitDiffPanel";
import PlanReviewPanel from "./PlanReviewPanel";

const STATUS_ICON: Record<string, string> = {
  pending: "\u25cb", in_progress: "\u25cf", completed: "\u2713", cancelled: "\u2717",
};
const TOOL_ICONS: Record<string, string> = {
  file_read: "R", file_edit: "E", file_write: "W", exec_run: ">",
  web_search: "@", web_fetch: "@",
  web_context7_resolve: "D", web_context7_query: "D",
};

type Section = "tasks" | "activity" | "plan" | "git" | "skills";

export default function StatusPanel(props: {
  tasks: () => TaskInfo[];
  recentEdits: () => string[];
  activityLog: () => ActivityEntry[];
  seed: string;
  loadActivityFromBackend?: () => Promise<void>;
  onTaskAction?: (action: "cancel" | "delete" | "ask", taskId: string, subject: string, description: string) => void;
  skillCatalog: () => SkillInfo[];
  activeSkillNames: () => string[];
}) {
  const { t } = useI18n();
  const [expanded, setExpanded] = createSignal<Section | null>(null);
  const [nowTick, setNowTick] = createSignal(Date.now());
  const timer = setInterval(() => setNowTick(Date.now()), 1000);

  // Load activity from backend when panel mounts
  onMount(() => {
    props.loadActivityFromBackend?.();
  });

  const formatActivity = (e: ActivityEntry): { icon: string; desc: string; detail: string } => {
    const name = e.tool_name;
    let args: Record<string, unknown> = {};
    try { args = JSON.parse(e.args || "{}"); } catch (_) {}
    const icon = TOOL_ICONS[name] || "?";
    let desc = name;
    let detail = e.summary || "";

    if (name.startsWith("file")) {
      const path = (args.path || args.new_path || args.source || args.dest || "") as string;
      const action = name === "edit" ? t().tool.activityLabel.modified : t().tool.activityLabel.read;
      desc = `${action}`;
      detail = path ? path.replace(/\\/g, "/").split("/").pop() || path : detail;
    } else if (name === "exec_run") {
      const cmd = (args.argv ? (args.argv as string[]).join(" ") : args.command || "") as string;
      desc = t().tool.activityLabel.executed;
      detail = cmd || detail;
    } else if (name.startsWith("web_")) {
      const q = (args.query || args.url || "") as string;
      desc = name === "web_fetch" ? t().tool.activityLabel.fetched : t().tool.activityLabel.searched;
      detail = String(q).substring(0, 80);
    } else if (name.startsWith("web_context7")) {
      desc = t().tool.activityLabel.fetched;
      detail = (args.query || args.name || "") as string;
    }

    if (detail.startsWith("[OK] ")) detail = detail.slice(4);
    if (detail.startsWith("[ERROR] ")) detail = detail.slice(8);
    if (detail.startsWith("[FAIL] ")) detail = detail.slice(7);
    return { icon, desc, detail };
  };

  // Panel resize
  let panelW = Number(localStorage.getItem("deepx:panel-w")) || 340;
  const setPanelW = (w: number) => {
    panelW = w;
    document.documentElement.style.setProperty("--panel-w", w + "px");
    document.documentElement.style.setProperty("--panel-foot", w + "px");
    localStorage.setItem("deepx:panel-w", String(w));
  };
  onMount(() => setPanelW(panelW));
  // Auto-expand PLAN section when PLAN.md changes
  onMount(() => {
    const unlisten = listen("plan-changed", () => setExpanded("plan"));
    onCleanup(() => { unlisten.then((fn) => fn()); });
  });
  onCleanup(() => clearInterval(timer));

  const elapsed = (ts: number) => {
    const _ = nowTick();
    const s = Math.floor((Date.now() - ts) / 1000);
    if (s < 60) return s + "s";
    if (s < 3600) return Math.floor(s / 60) + "m";
    return Math.floor(s / 3600) + "h";
  };

  const toggle = (section: Section) => {
    setExpanded((prev) => (prev === section ? null : section));
  };
  const isOpen = (section: Section) => expanded() === section;

  // O(1) lookup for active skill names — avoids re-scanning on every render
  const activeSet = createMemo(() => new Set(props.activeSkillNames()));

  // ── Scroll preservation ──
  let skillsBodyRef!: HTMLDivElement;
  let skillsScrollTop = 0;
  let skillsScrollHeight = 0;
  createEffect((prev: number | undefined) => {
    const _active = activeSet(); // track changes
    const el = skillsBodyRef;
    if (!el || !isOpen("skills")) return prev;
    if (prev === undefined) return 0;
    // Restore scroll after DOM update. If content shrank (unload),
    // clamp scrollTop so we don't overshoot.
    if (el.scrollHeight !== skillsScrollHeight) {
      el.scrollTop = Math.min(skillsScrollTop, Math.max(0, el.scrollHeight - el.clientHeight));
    }
    return _active ? 1 : 0;
  });

  // Save scroll before any re-render triggered by activeSet/click
  function saveSkillsScroll() {
    const el = skillsBodyRef;
    if (el) {
      skillsScrollTop = el.scrollTop;
      skillsScrollHeight = el.scrollHeight;
    }
  }

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
        <span>{t().status.title}</span>
      </div>
      <div class="status-panel-body">

        {/* ── Skills ── */}
        <div class={`status-accordion${isOpen("skills") ? " expanded" : ""}`}>
          <div class="status-tile" onClick={() => toggle("skills")}>
            <span class={`status-tile-arrow${isOpen("skills") ? " open" : ""}`}>▶</span>
            <span class="status-tile-label">{t().status.skills}</span>
            <Show when={props.activeSkillNames().length > 0}>
              <span class="status-tile-badge">{props.activeSkillNames().length}</span>
            </Show>
          </div>
          <div class={`status-accordion-body${isOpen("skills") ? " expanded" : ""}`} ref={skillsBodyRef}>
            <div class="skills-toolbar">
              <button class="skills-btn-reload" onClick={() => invoke("cmd_reload_skills", { seed: props.seed }).catch(() => {})}>
                ↻ {t().status.skillReload}
              </button>
            </div>
            <Show when={props.skillCatalog().length > 0} fallback={<div class="status-empty">{t().status.noSkills}</div>}>
              <For each={props.skillCatalog()}>
                {(skill) => {
                  const isActive = () => activeSet().has(skill.name);
                  return (
                    <div class={`status-row${isActive() ? " status-active" : ""}`} data-skill={skill.name}>
                      <span class="status-row-icon" style={{"font-size": "14px"}}>{isActive() ? "✓" : "○"}</span>
                      <div class="status-row-info">
                        <span class="status-row-title">{skill.name}</span>
                        <span class="status-row-desc" style={{"font-size": "10px"}}>
                          {skill.scope === "project" ? t().status.skillProject : t().status.skillUser}: {skill.source}
                        </span>
                      </div>
                      <Show when={isActive()}>
                        <button
                          class="skills-btn-unload"
                          onClick={() => { saveSkillsScroll(); invoke("cmd_unload_skill", { seed: props.seed, name: skill.name }).catch(() => {}); }}
                          title={t().status.skillUnload}
                        >✕</button>
                      </Show>
                      <Show when={!isActive()}>
                        <button
                          class="skills-btn-activate"
                          onClick={() => { saveSkillsScroll(); invoke("cmd_activate_skill", { seed: props.seed, name: skill.name }).catch(() => {}); }}
                          title={t().status.skillActivate}
                        >+</button>
                      </Show>
                    </div>
                  );
                }}
              </For>
            </Show>
          </div>
        </div>

        {/* ── Tasks ── */}
        <div class={`status-accordion${isOpen("tasks") ? " expanded" : ""}`}>
          <div class="status-tile" onClick={() => toggle("tasks")}>
            <span class={`status-tile-arrow${isOpen("tasks") ? " open" : ""}`}>▶</span>
            <span class="status-tile-label">{t().status.tasks}</span>
            <Show when={props.tasks().length > 0}>
              <span class="status-tile-badge">{props.tasks().filter((t) => t.status === "completed").length}/{props.tasks().length}</span>
            </Show>
          </div>
          <div class={`status-accordion-body${isOpen("tasks") ? " expanded" : ""}`}>
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
                          <button class="task-btn task-btn-cancel" onClick={() => props.onTaskAction!("cancel", task.id, task.subject, task.description)} title={t().status.taskCancel}>✕</button>
                        </Show>
                        <Show when={task.status === "completed" || task.status === "cancelled"}>
                          <button class="task-btn task-btn-delete" onClick={() => props.onTaskAction!("delete", task.id, task.subject, task.description)} title={t().status.taskDelete}>🗑</button>
                        </Show>
                        <button class="task-btn task-btn-ask" onClick={() => props.onTaskAction!("ask", task.id, task.subject, task.description)} title={t().status.taskAsk}>?</button>
                      </div>
                    </Show>
                  </div>
                )}
              </For>
            </Show>
          </div>
        </div>

        {/* ── Activity ── */}
        <div class={`status-accordion${isOpen("activity") ? " expanded" : ""}`}>
          <div class="status-tile" onClick={() => toggle("activity")}>
            <span class={`status-tile-arrow${isOpen("activity") ? " open" : ""}`}>▶</span>
            <span class="status-tile-label">{t().status.activity}</span>
            <Show when={props.activityLog().length > 0}>
              <span class="status-tile-badge">{props.activityLog().length}</span>
            </Show>
          </div>
          <div class={`status-accordion-body${isOpen("activity") ? " expanded" : ""}`}>
            <Show when={props.activityLog().length > 0} fallback={<div class="status-empty">{t().status.noActivity}</div>}>
               <For each={props.activityLog()}>
                 {(entry) => {
                   const fmt = formatActivity(entry);
                   return (
                   <div class={`status-row status-${entry.success ? "success" : "fail"}`}>
                     <span class="status-row-icon activity-icon">{fmt.icon}</span>
                     <div class="status-row-info">
                       <span class="status-row-title">{fmt.desc}</span>
                       <span class="status-row-desc">{fmt.detail}</span>
                     </div>
                     <span class="status-row-time">{entry.time ? entry.time.split(" ").pop()?.slice(0, 5) : ""}</span>
                   </div>
                 )}}
               </For>
            </Show>
          </div>
        </div>

        {/* ── PLAN Review ── */}
        <div class={`status-accordion${isOpen("plan") ? " expanded" : ""}`}>
          <div class="status-tile" onClick={() => toggle("plan")}>
            <span class={`status-tile-arrow${isOpen("plan") ? " open" : ""}`}>▶</span>
            <span class="status-tile-label">{t().planReview?.title ?? "PLAN Review"}</span>
          </div>
          <div class={`status-accordion-body${isOpen("plan") ? " expanded" : ""}`}>
            <PlanReviewPanel seed={props.seed} />
          </div>
        </div>

        {/* ── Git Changes ── */}
        <div class={`status-accordion${isOpen("git") ? " expanded" : ""}`}>
          <div class="status-tile" onClick={() => toggle("git")}>
            <span class={`status-tile-arrow${isOpen("git") ? " open" : ""}`}>▶</span>
            <span class="status-tile-label">{t().status.gitChanges}</span>
          </div>
          <div class={`status-accordion-body${isOpen("git") ? " expanded" : ""}`}>
            <GitDiffPanel seed={props.seed} />
          </div>
        </div>

      </div>
    </div>
  );
}