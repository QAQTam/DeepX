import { createSignal, onMount, onCleanup, Show, For, createEffect } from "solid-js";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { createChatStore, type ToolCallDef, type RoundBlock, type ToolResultDef, type SessionMeta } from "./store/chat";
import ChatView from "./components/ChatView";
import SettingsView from "./components/SettingsView";
import StatusBar from "./components/StatusBar";
import { createI18n, I18nCtx, type Lang } from "./i18n";
import en from "./i18n/en";

type View = "chat" | "settings";

const LS_KEY = "dsx:seed";

export default function App() {
  const i18n = createI18n(((localStorage.getItem("dsx:lang") ?? "en") as Lang));
  const chat = createChatStore();
  const [view, setView] = createSignal<View>("chat");
  const [configLang, setConfigLang] = createSignal<Lang>("en");
  const [sessions, setSessions] = createSignal<SessionMeta[]>([]);
  let unlisten: (() => void) | undefined;

  // Load session list from disk
  async function refreshSessions() {
    try {
      const raw = await invoke<string>("cmd_list_sessions");
      const list: SessionMeta[] = JSON.parse(raw);
      list.sort((a, b) => b.updated_at - a.updated_at);
      setSessions(list);
    } catch (e) { console.error(e); }
  }

  // Resume an existing session (save seed, reload)
  async function resumeSession(seed: string) {
    try {
      localStorage.setItem(LS_KEY, seed);
      await invoke("cmd_set_active_session", { seed });
      window.location.reload();
    } catch (e) { console.error(e); }
  }

  // Start a new session
  async function newSession() {
    localStorage.removeItem(LS_KEY);
    try { await invoke("cmd_set_active_session", { seed: "" }); } catch (_) {}
    window.location.reload();
  }

  // Load persisted session data on resume
  async function loadPersistedSession() {
    const savedSeed = localStorage.getItem(LS_KEY);
    if (!savedSeed) return false;
    try {
      const raw = await invoke<string>("cmd_load_session", { seed: savedSeed });
      const count = chat.loadSessionFromData(raw);
      if ((count ?? 0) > 0) {
        chat.handleSessionCreated(savedSeed);
        return true;
      }
    } catch (e) { console.error(e); }
    return false;
  }

  onMount(async () => {
    await refreshSessions();
    const restored = await loadPersistedSession();

    if (!restored) {
      try { await invoke("cmd_create_session"); } catch (e) { console.error(e); }
    }

    try {
      unlisten = await listen<Record<string, unknown>>("agent-event", (e) => {
        const p = e.payload;
        switch (p.type as string) {
          case "turn_start": chat.handleTurnStart((p.turn_id ?? "") as string, (p.user_text ?? "") as string); break;
          case "round_delta": chat.handleRoundDelta((p.turn_id ?? "") as string, (p.round_num ?? 0) as number, (p.kind ?? "") as string, (p.delta ?? "") as string); break;
          case "round_complete": chat.handleRoundComplete((p.turn_id ?? "") as string, (p.round_num ?? 0) as number, p.thinking as string | undefined, p.answer as string | undefined, p.tool_calls as ToolCallDef[] | undefined, p.blocks as RoundBlock[] | undefined); break;
          case "tool_results": chat.handleToolResults((p.turn_id ?? "") as string, (p.round_num ?? 0) as number, p.results as ToolResultDef[]); break;
          case "turn_end": chat.handleTurnEnd((p.turn_id ?? "") as string, p); break;
          case "session_created": {
            chat.handleSessionCreated(p.seed as string);
            localStorage.setItem(LS_KEY, p.seed as string);
            refreshSessions();
            break;
          }
          case "session_restored": if (p.seed) { chat.handleSessionCreated(p.seed as string); localStorage.setItem(LS_KEY, p.seed as string); } break;
          case "debug_snapshot": chat.handleDebugSnapshot(p); break;
          case "done": chat.setInputDisabled(false); break;
          case "cancelled": chat.handleCancelled(); break;
          case "error": chat.handleError((p.message ?? "Unknown error") as string); break;
        }
      });
    } catch (e) { console.error(e); }
  });

  onCleanup(() => unlisten?.());

  const t = () => i18n.t() ?? en;
  function switchLang(l: Lang) { i18n.setLang(l); setConfigLang(l); localStorage.setItem("dsx:lang", l); localStorage.setItem("dsx:lang", l); }

  const isActive = (seed: string) => chat.sessionInfo.seed === seed;
  const activeSeed = () => chat.sessionInfo.seed;

  return (
    <I18nCtx.Provider value={{ t: i18n.t, lang: () => i18n.lang(), setLang: switchLang }}>
      <div class="app-container">
        <aside class="sidebar">
          <div class="sidebar-brand"><span class="sidebar-logo">{">"}</span><span class="sidebar-title">{t().app.title}</span></div>
          <nav class="sidebar-nav">
            <button class={`sidebar-btn ${view() === "chat" ? "active" : ""}`} onClick={() => setView("chat")} title={t().nav.chat}>
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>
              <span>{t().nav.chat}</span>
            </button>
            <button class={`sidebar-btn ${view() === "settings" ? "active" : ""}`} onClick={() => setView("settings")} title={t().nav.settings}>
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="3"/><path d="M12 1v2M12 21v2M4.22 4.22l1.42 1.42M18.36 18.36l1.42 1.42M1 12h2M21 12h2M4.22 19.78l1.42-1.42M18.36 5.64l1.42-1.42"/></svg>
              <span>{t().nav.settings}</span>
            </button>
          </nav>

          {/* Session list */}
          <div class="sidebar-section-label">{t().session.resume}</div>
          <div class="sidebar-sessions">
            <For each={sessions()}>
              {(s) => (
                <button
                  class={`sidebar-session-item ${isActive(s.seed) ? "active" : ""}`}
                  onClick={() => resumeSession(s.seed)}
                  title={s.last_summary || s.seed}
                >
                  <span class="session-dot" />
                  <span class="session-info">
                    <span class="session-summary">{s.last_summary || s.seed.substring(0, 8)}</span>
                    <span class="session-meta">{formatDate(s.updated_at)} · {s.message_count} {t().session.messages}</span>
                  </span>
                </button>
              )}
            </For>
          </div>

          <div class="sidebar-spacer" />
          <div class="sidebar-new-session">
            <button onClick={newSession} title={t().session.new}>+ {t().session.new}</button>
          </div>
        </aside>

        <main class="main-content">
          <Show when={view() === "chat"} fallback={<SettingsView lang={configLang} onLangChange={switchLang} onClose={() => setView("chat")} />}>
            <ChatView chat={chat} />
          </Show>
        </main>

        <StatusBar model={t().chat.modelLabel} sessionSeed={chat.sessionInfo.seed} contextTokens={chat.sessionInfo.contextTokens} contextLimit={chat.sessionInfo.contextLimit} sessionTokens={chat.sessionInfo.sessionTokens} isStreaming={chat.isStreaming()} error={chat.error()} />
      </div>
    </I18nCtx.Provider>
  );
}

function formatDate(epoch: number): string {
  const d = new Date(epoch * 1000);
  const now = new Date();
  const diff = now.getTime() - d.getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  return d.toLocaleDateString();
}
