import { For, createMemo, Show } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "../i18n";
import type { SessionMeta } from "../store/chat";

interface StartupViewProps {
  sessions: SessionMeta[];
  onResume: (seed: string) => void;
  onSend?: (text: string) => void;
  showHeatmap?: boolean;
}

/** Compute daily activity counts from sessions. Returns Map<"YYYY-MM-DD", count>. */
function computeActivity(sessions: SessionMeta[]): Map<string, number> {
  const map = new Map<string, number>();
  for (const s of sessions) {
    const d = new Date(Number(s.created_at) * 1000);
    const key = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
    map.set(key, (map.get(key) ?? 0) + (s.message_count || 1));
  }
  return map;
}

/** Generate the last N days as "YYYY-MM-DD" strings. */
function lastNDays(n: number): string[] {
  const days: string[] = [];
  const now = new Date();
  for (let i = n - 1; i >= 0; i--) {
    const d = new Date(now);
    d.setDate(d.getDate() - i);
    days.push(`${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`);
  }
  return days;
}

/** Map count to a CSS class for color intensity. */
function levelClass(count: number): string {
  if (count === 0) return "hm-l0";
  if (count <= 3) return "hm-l1";
  if (count <= 8) return "hm-l2";
  if (count <= 20) return "hm-l3";
  return "hm-l4";
}

export default function StartupView(props: StartupViewProps) {
  const { t } = useI18n();
  let textareaRef!: HTMLTextAreaElement;

  const activity = createMemo(() => computeActivity(props.sessions));
  const days30 = createMemo(() => lastNDays(30));

  async function handleSend(text: string) {
    if (props.onSend) { props.onSend(text); return; }
    try { await invoke("cmd_send_message", { seed: "", text }); } catch (e) { console.error(e); }
  }
  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); submit(); }
  }
  function submit() {
    const text = textareaRef.value.trim();
    if (!text) return;
    handleSend(text);
    textareaRef.value = "";
    textareaRef.style.height = "auto";
  }
  function autoResize() {
    textareaRef.style.height = "auto";
    textareaRef.style.height = Math.min(textareaRef.scrollHeight, 160) + "px";
  }
  function formatDate(epoch: number): string {
    const d = new Date(epoch * 1000);
    const now = new Date();
    const diff = now.getTime() - d.getTime();
    const mins = Math.floor(diff / 60000);
    if (mins < 60) return mins + t().time.mSuffix;
    const hours = Math.floor(mins / 60);
    if (hours < 24) return hours + t().time.hSuffix;
    return d.toLocaleDateString();
  }

  const recentSessions = () => props.sessions.slice(0, 5);

  return (
    <div class="startup-view">
      <div class="startup-center">
        <div class="startup-logo">{">"}</div>
        <h1 class="startup-title">{t().app.title}</h1>
        <p class="startup-subtitle">{t().app.subtitle}</p>

        {/* ── Contribution Heatmap (home page only) ── */}
        <Show when={props.showHeatmap}>
          <div class="heatmap-card">
            <div class="heatmap-header">
              <span class="heatmap-label">{t().startup.activity}</span>
              <span class="heatmap-total">{props.sessions.length} {t().startup.sessions}</span>
            </div>
            <div class="heatmap-grid">
              <For each={days30()}>
                {(day) => {
                  const count = activity().get(day) ?? 0;
                  return (
                    <div
                      class={`heatmap-cell ${levelClass(count)}`}
                      title={`${day}: ${count} messages`}
                    />
                  );
                }}
              </For>
            </div>
            <div class="heatmap-legend">
              <span>{t().startup.less}</span>
              <span class="heatmap-cell hm-l0" />
              <span class="heatmap-cell hm-l1" />
              <span class="heatmap-cell hm-l2" />
              <span class="heatmap-cell hm-l3" />
              <span class="heatmap-cell hm-l4" />
              <span>{t().startup.more}</span>
            </div>
          </div>
        </Show>

        <div class="startup-input-wrap">
          <textarea
            ref={textareaRef}
            rows={2}
            placeholder={t().chat.placeholder}
            onKeyDown={handleKeyDown}
            onInput={autoResize}
            autofocus
          />
          <button class="startup-send" onClick={submit} title={t().chat.send}>
            <svg width="18" height="18" viewBox="0 0 16 16"><path d="M2 2l12 6-12 6 3-6-3-6z" fill="currentColor"/></svg>
          </button>
        </div>
        <p class="startup-hint">{t().session.startupHint}</p>

        {props.sessions.length > 0 && (
          <div class="startup-recent">
            <div class="startup-recent-label">{t().session.recent}</div>
            <div class="startup-recent-list">
              <For each={recentSessions()}>
                {(s) => (
                  <button class="startup-recent-item" onClick={() => props.onResume(s.seed)}>
                    <span class="startup-recent-summary">{s.last_summary || s.seed.substring(0, 8)}</span>
                    <span class="startup-recent-meta">{formatDate(Number(s.updated_at))} · {s.turn_count || s.message_count} {t().session.turns}</span>
                  </button>
                )}
              </For>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
