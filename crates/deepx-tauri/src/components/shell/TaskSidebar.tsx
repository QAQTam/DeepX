import { For } from "solid-js";
import type { SessionMeta } from "../../lib/types";

export function taskTitle(session: SessionMeta, dashboardTitle?: string): string {
  return dashboardTitle?.trim() || session.last_summary?.trim() || session.seed.slice(0, 8);
}

export default function TaskSidebar(props: {
  sessions: SessionMeta[];
  activeSeed: string;
  titles?: Record<string, string>;
  onNew: () => void;
  onOpen: (seed: string) => void;
  onDelete: (seed: string) => void;
  onSkills: () => void;
  onSettings: () => void;
}) {
  return <aside class="task-sidebar" data-task-sidebar>
    <div class="task-sidebar-brand"><span>&gt;</span><strong>DeepX</strong></div>
    <nav class="task-sidebar-primary">
      <button onClick={props.onNew}>＋ 新建任务</button>
      <button onClick={props.onSkills}>◇ 技能</button>
      <button onClick={props.onSettings}>⚙ 设置</button>
    </nav>
    <div class="task-sidebar-label">任务</div>
    <div class="task-sidebar-list">
      <For each={props.sessions}>{session =>
        <div class={`task-row ${session.seed === props.activeSeed ? "active" : ""}`} data-task-session>
          <button class="task-row-main" onClick={() => props.onOpen(session.seed)}>
            <span class={`task-state ${session.running ? "running" : ""}`} />
            <span>{taskTitle(session, props.titles?.[session.seed])}</span>
          </button>
          <button class="task-row-menu" aria-label="删除任务" onClick={() => props.onDelete(session.seed)}>×</button>
        </div>
      }</For>
    </div>
  </aside>;
}
