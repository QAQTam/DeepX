import { Show, createEffect, createSignal, onCleanup } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "../i18n";

type GoalStatus = { objective: string; status: "active" | "paused" | "completed" | "stopped"; current_id?: string; current_title?: string; completed: number; total: number; paused_reason?: string };

export default function GoalStatusStrip(props: { seed: string }) {
  const { t } = useI18n();
  const [goal, setGoal] = createSignal<GoalStatus | null>(null);
  const [busy, setBusy] = createSignal(false);
  async function refresh() {
    try { const raw = await invoke<string>("cmd_get_goal_status", { seed: props.seed }); setGoal(raw === "null" ? null : JSON.parse(raw) as GoalStatus); }
    catch { setGoal(null); }
  }
  async function action(action: "pause" | "resume" | "stop") {
    if (busy()) return;
    setBusy(true);
    try { await invoke("cmd_goal_action", { seed: props.seed, action }); }
    finally { setBusy(false); await refresh(); }
  }
  createEffect(() => props.seed, () => { void refresh(); const timer = window.setInterval(() => void refresh(), 2_500); onCleanup(() => window.clearInterval(timer)); });
  return <Show when={goal()}>{item => <section class={`goal-status-strip ${item().status}`} aria-label={t().goal.title}>
    <div class="goal-status-copy"><span class="goal-status-label">◉ {t().goal.title} · {item().status === "active" ? t().goal.active : item().status === "paused" ? t().goal.paused : t().goal.completed}</span><strong>{item().objective}</strong><small>{t().goal.step} {Math.min(item().completed + 1, item().total)}/{item().total}{item().current_title ? ` · ${item().current_id}: ${item().current_title}` : ""}{item().paused_reason ? ` · ${item().paused_reason}` : ""}</small></div>
    <div class="goal-status-actions"><Show when={item().status === "active"}><button disabled={busy()} onClick={() => void action("pause")}>{t().goal.pause}</button></Show><Show when={item().status === "paused"}><button disabled={busy()} onClick={() => void action("resume")}>{t().goal.resume}</button></Show><Show when={item().status === "active" || item().status === "paused"}><button disabled={busy()} class="danger" onClick={() => void action("stop")}>{t().goal.stop}</button></Show></div>
  </section>}</Show>;
}
