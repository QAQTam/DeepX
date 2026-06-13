import { createSignal, onMount, onCleanup, Show, For } from "solid-js";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { createChatStore, type ToolCallDef, type RoundBlock, type ToolResultDef, type SessionMeta } from "./store/chat";
import ChatView from "./components/ChatView";
import SettingsView from "./components/SettingsView";
import InfoBar from "./components/InfoBar";
import StatusPanel from "./components/StatusPanel";
import { createI18n, I18nCtx, type Lang } from "./i18n";
import en from "./i18n/en";

type View = "chat" | "settings";

const LS_KEY = "deepx:seed";

export default function App() {
  const i18n = createI18n(((localStorage.getItem("deepx:lang") ?? "en") as Lang));
  const chat = createChatStore();
  const [view, setView] = createSignal<View>("chat");
  const [configLang, setConfigLang] = createSignal<Lang>("en");
  const [sessions, setSessions] = createSignal<SessionMeta[]>([]);
  let unlisten: (() => void) | undefined;

  async function refreshSessions() {
    try {
      const raw = await invoke<string>("cmd_list_sessions");
      const list: SessionMeta[] = JSON.parse(raw);
      list.sort((a, b) => b.updated_at - a.updated_at);
      setSessions(list);
    } catch (e) { console.error(e); }
  }

  async function resumeSession(seed: string) {
    try {
      localStorage.setItem(LS_KEY, seed);
      await invoke("cmd_set_active_session", { seed });
      window.location.reload();
    } catch (e) { console.error(e); }
  }

  async function deleteSession(seed: string) {
    try {
      await invoke("cmd_delete_session", { seed });
      if (chat.sessionInfo.seed === seed) {
        localStorage.removeItem(LS_KEY);
        await invoke("cmd_set_active_session", { seed: "" });
        await invoke("cmd_create_session");
      }
      await refreshSessions();
    } catch (e) { console.error(e); }
  }

  async function newSession() {
    localStorage.removeItem(LS_KEY);
    try { await invoke("cmd_set_active_session", { seed: "" }); } catch (_) {}
    window.location.reload();
  }

  onMount(async () => {
    try {
      const raw = await invoke<string>("cmd_load_config");
      const cfg = JSON.parse(raw);
      if (cfg.model) chat.handleDashboard({ model: cfg.model });
      if (cfg.lang && (cfg.lang === "en" || cfg.lang === "zh")) {
        const cl = cfg.lang as Lang;
        i18n.setLang(cl);
        setConfigLang(cl);
        localStorage.setItem("deepx:lang", cl);
      }
    } catch (_) {}
    // Set up event listener FIRST
    try { unlisten = await listen<Record<string, unknown>>("agent-event", (e) => {
      const p = e.payload;
      switch (p.type as string) {
        case "turn_start": chat.handleTurnStart((p.turn_id ?? "") as string, (p.user_text ?? "") as string); break;
        case "round_delta": chat.handleRoundDelta((p.turn_id ?? "") as string, (p.round_num ?? 0) as number, (p.kind ?? "") as string, (p.delta ?? "") as string); break;
        case "round_complete": chat.handleRoundComplete((p.turn_id ?? "") as string, (p.round_num ?? 0) as number, p.thinking as string | undefined, p.answer as string | undefined, p.tool_calls as ToolCallDef[] | undefined, p.blocks as RoundBlock[] | undefined); break;
        case "tool_results": chat.handleToolResults((p.turn_id ?? "") as string, (p.round_num ?? 0) as number, p.results as ToolResultDef[]); break;
        case "turn_end": chat.handleTurnEnd((p.turn_id ?? "") as string, p); break;
        case "session_created": chat.handleSessionCreated(p.seed as string); localStorage.setItem(LS_KEY, p.seed as string); refreshSessions(); break;
        case "session_restored": if (p.seed) { chat.handleSessionCreated(p.seed as string); localStorage.setItem(LS_KEY, p.seed as string); if (p.turns) { chat.loadTurnsFromRestore(p.turns as Array<{ turn_id: string; user_text: string; rounds: Array<{ round_num: number; thinking?: string; answer?: string; tool_calls: ToolCallDef[]; tool_results: ToolResultDef[] }> }>); } } break;
        case "dashboard": chat.handleDashboard(p); break;
        case "done": chat.setInputDisabled(false); break;
        case "cancelled": chat.handleCancelled(); break;
        case "error": chat.handleError((p.message ?? "Unknown error") as string); break;
        case "audit_record": chat.handleAuditRecord({ tool_name: (p.tool_name ?? "") as string, result_summary: (p.result_summary ?? "") as string, success: (p.success ?? false) as boolean }); break;
      }
    }); } catch (e) { console.error(e); }

    // Load sessions + handle initial state
    await refreshSessions();
    const savedSeed = localStorage.getItem(LS_KEY);
    if (!savedSeed) {
      try { await invoke("cmd_create_session"); } catch (e) { console.error(e); }
    } else {
      try {
        const raw = await invoke<string>("cmd_load_session", { seed: savedSeed });
        chat.loadSessionFromData(raw);
        chat.handleSessionCreated(savedSeed);
        } catch (e) {
          console.error("load session failed, server will handle recovery:", e);
          chat.setInputDisabled(false);
        }
    }
  });

  onCleanup(() => unlisten?.());

  const t = () => i18n.t() ?? en;
  async function switchLang(l: Lang) { i18n.setLang(l); setConfigLang(l); localStorage.setItem("deepx:lang", l); try { await invoke("cmd_save_config", { apiKey: "", model: "", baseUrl: "", providerId: "", endpoint: "", maxTokens: 0, contextLimit: 0, reasoningEffort: "", lang: l }); } catch (e) { console.error(e); } }

  const isActive = (seed: string) => chat.sessionInfo.seed === seed;

  return (
    <I18nCtx.Provider value={{ t: i18n.t, lang: () => i18n.lang(), setLang: switchLang }}>
      <div class="app-container">
        <aside class="sidebar frost-panel">
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
          <div class="sidebar-section-label">{t().session.resume}</div>
          <div class="sidebar-sessions">
            <For each={sessions()}>
              {(s) => (
                <button class={`sidebar-session-item ${isActive(s.seed) ? "active" : ""}`} onClick={() => resumeSession(s.seed)} title={s.last_summary || s.seed}>
                  <span class="session-dot" />
                  <span class="session-info">
                    <span class="session-summary">{s.last_summary || s.seed.substring(0, 8)}</span>
                    <span class="session-meta">{formatDate(s.updated_at)} · {s.message_count} {t().session.messages}</span>
                  </span>
                  <span
                    class="session-delete-btn"
                    onClick={(e) => { e.stopPropagation(); deleteSession(s.seed); }}
                    title="Delete session"
                  >
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                      <path d="M18 6L6 18M6 6l12 12" />
                    </svg>
                  </span>
                </button>
              )}
            </For>
          </div>
          <div class="sidebar-spacer" />
          <div class="sidebar-new-session"><button onClick={newSession} title={t().session.new}>+ {t().session.new}</button></div>
        </aside>
        <main class="main-content">
          <Show when={view() === "chat"} fallback={<SettingsView lang={configLang} onLangChange={switchLang} onClose={() => setView("chat")} />}>
            <ChatView chat={chat} />
            <StatusPanel tasks={chat.tasks} recentEdits={chat.recentEdits} activityLog={chat.activityLog} />
          </Show>
        </main>
        
      </div>
    </I18nCtx.Provider>
  );
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