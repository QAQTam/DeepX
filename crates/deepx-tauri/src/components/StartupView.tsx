import { For } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "../i18n";
import type { SessionMeta } from "../store/chat";

interface StartupViewProps {
  sessions: SessionMeta[];
  onResume: (seed: string) => void;
}

export default function StartupView(props: StartupViewProps) {
  const { t } = useI18n();
  let textareaRef!: HTMLTextAreaElement;

  async function handleSend(text: string) {
    try {
      await invoke("cmd_send_message", { text });
    } catch (e) {
      console.error("send_message error:", e);
    }
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      submit();
    }
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
    if (mins < 60) return mins + "m ago";
    const hours = Math.floor(mins / 60);
    if (hours < 24) return hours + "h ago";
    return d.toLocaleDateString();
  }

  const recentSessions = () => props.sessions.slice(0, 5);

  return (
    <div class="startup-view">
      <div class="startup-center">
        <div class="startup-logo">{">"}</div>
        <h1 class="startup-title">{t().app.title}</h1>
        <p class="startup-subtitle">{t().app.subtitle}</p>
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
                    <span class="startup-recent-meta">{formatDate(s.updated_at)} · {s.message_count} {t().session.messages}</span>
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
