import { createSignal, onMount, onCleanup, Show, For, Switch, Match } from "solid-js";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { createChatStore, type ToolCallDef, type RoundBlock, type ToolResultDef, type SessionMeta } from "./store/chat";
import type { Agent2Ui } from "@/lib/types";
import type { SlashCommand } from "./components/SlashMenu";
import ChatView from "./components/ChatView";
import StartupView from "./components/StartupView";
import SettingsView from "./components/SettingsView";
import InfoBar from "./components/InfoBar";
import StatusPanel from "./components/StatusPanel";
import TokenChart from "./components/TokenChart";
import "./styles/git-diff-panel.css";
import "./styles/context-panel.css";
import "./styles/slash-menu.css";
import "./styles/permission-dialog.css";
import "./styles/changelog.css";
import { ToastContainer, createToastCtrl, type ToastCtrl } from "./components/Toast";
import PermissionDialog, { type PermissionRequest } from "./components/PermissionDialog";
import ChangelogModal from "./components/ChangelogModal";
import { createI18n, I18nCtx, type Lang } from "./i18n";
import en from "./i18n/en";

type View = "home" | "chat" | "settings";
export type ThemeMode = "system" | "light" | "dark" | "dark-gray";

const LS_KEY = "deepx:seed";
const LS_THEME = "deepx:theme";

/** Resolve a ThemeMode to the actual data-theme value to apply. */
function resolveTheme(mode: ThemeMode): "light" | "dark" | "dark-gray" {
  if (mode !== "system") return mode;
  if (typeof window !== "undefined" && window.matchMedia?.("(prefers-color-scheme: dark)").matches) {
    return "dark-gray";
  }
  return "light";
}

function applyTheme(mode: ThemeMode) {
  document.documentElement.setAttribute("data-theme", resolveTheme(mode));
}

// ── Multi-session store registry ──
// Each open session gets its own ChatStore, keyed by seed.
type ChatStore = ReturnType<typeof createChatStore>;

export default function App() {
  const i18n = createI18n(((localStorage.getItem("deepx:lang") ?? "en") as Lang));
  const [view, setView] = createSignal<View>("home");
  const [configLang, setConfigLang] = createSignal<Lang>("en");
  const [sessions, setSessions] = createSignal<SessionMeta[]>([]);
  // Active session seed — drives which ChatStore is displayed
  const [activeSeed, setActiveSeed] = createSignal<string>("");
  // Has the user explicitly chosen a session?
  const [hasChosenSession, setHasChosenSession] = createSignal(false);
  // Workspace draft — survives session switches so the sidebar input keeps its value.
  const [workspaceDraft, setWorkspaceDraft] = createSignal(localStorage.getItem("deepx:workspace") ?? "");
  const [version, setVersion] = createSignal("");
  const [sidebarW, setSidebarW] = createSignal(
    Number(localStorage.getItem("deepx:sidebar-w")) || 220
  );

  // Apply sidebar width to CSS variable
  onMount(() => {
    document.documentElement.style.setProperty("--sidebar-w", sidebarW() + "px");
    // Fetch version
    invoke<string>("cmd_get_version").then(setVersion).catch(() => {});
  });
  const [theme, setTheme] = createSignal<ThemeMode>("system");
  const [refreshKey, setRefreshKey] = createSignal(0); // bump to refresh TokenChart
  const [permissionRequest, setPermissionRequest] = createSignal<PermissionRequest | null>(null);
  const [showChangelog, setShowChangelog] = createSignal(false);

  // ── Toast notifications (disconnect warnings, errors) ──
  const toastCtrl: ToastCtrl = createToastCtrl();

  // Registry of open session ChatStores
  const chatStores = new Map<string, ChatStore>();
  // Per-seed unlisten functions for event listeners
  const unlistenMap = new Map<string, () => void>();
  // Pending store creations — deduplicate concurrent getOrCreateChatStore calls
  const pendingStores = new Map<string, Promise<ChatStore>>();
  let unlistenTheme: (() => void) | undefined;

  /** Get or create a ChatStore for the given seed. Also sets up event listener.
   * Returns a Promise that resolves when the listener is ready.
   * Deduplicates concurrent calls for the same seed. */
  async function getOrCreateChatStore(seed: string): Promise<ChatStore> {
    let store = chatStores.get(seed);
    if (store) return store;
    // If a creation is already in-flight for this seed, wait for it
    const pending = pendingStores.get(seed);
    if (pending) return pending;
    // Start creation and store the promise for deduplication
    const creation = (async () => {
      const s = createChatStore(seed);
      chatStores.set(seed, s);
      // Subscribe to per-seed agent events
      const eventName = `agent-${seed}-event`;
      await listen<Record<string, unknown>>(eventName, (e) => {
        handleAgentEvent(s, e.payload, seed);
      }).then(unlisten => {
        unlistenMap.set(seed, unlisten);
      }).catch(console.error);
      return s;
    })();
    pendingStores.set(seed, creation);
    try {
      return await creation;
    } finally {
      pendingStores.delete(seed);
    }
  }

  /** Current active ChatStore (derived from activeSeed). */
  function activeChat(): ChatStore | undefined {
    const seed = activeSeed();
    if (!seed) return undefined;
    return chatStores.get(seed);
  }

  /** Handle incoming agent events for a specific store. */
  function handleAgentEvent(chat: ChatStore, p: Record<string, unknown>, listenerSeed: string) {
    switch (p.type as string) {
      case "ready": break;
      case "turn_start": chat.handleTurnStart((p.turn_id ?? "") as string, (p.user_text ?? "") as string); break;
      case "round_delta": chat.handleRoundDelta((p.turn_id ?? "") as string, (p.round_num ?? 0) as number, (p.kind ?? "") as string, (p.delta ?? "") as string); break;
      case "tool_call_preview": chat.handleToolCallPreview((p.turn_id ?? "") as string, (p.round_num ?? 0) as number, (p.index ?? 0) as number, (p.id ?? "") as string, (p.name ?? "") as string, (p.args_so_far ?? "") as string); break;
      case "round_complete": chat.handleRoundComplete((p.turn_id ?? "") as string, (p.round_num ?? 0) as number, p.thinking as string | undefined, p.answer as string | undefined, p.tool_calls as ToolCallDef[] | undefined, p.blocks as RoundBlock[] | undefined); break;
      case "tool_results": chat.handleToolResults((p.turn_id ?? "") as string, (p.round_num ?? 0) as number, p.results as ToolResultDef[]); break;
      case "plan_changed": {
        // Forward to PlanReviewPanel via DOM custom event
        window.dispatchEvent(new CustomEvent("plan-submitted", { detail: { seed: listenerSeed } }));
        break;
      }
      case "turn_end": chat.handleTurnEnd((p.turn_id ?? "") as string, p); if (listenerSeed === activeSeed()) setRefreshKey((k) => k + 1); break;
      case "session_created": {
        const evtSeed = p.seed as string;
        chat.clearTurns();
        chat.handleSessionCreated(evtSeed);
        localStorage.setItem(LS_KEY, evtSeed);
        // Sync workspace from backend to draft
        invoke<string>("cmd_get_workspace", { seed: evtSeed }).then(ws => {
          chat.setWorkspace(ws);
          if (evtSeed === activeSeed()) { setWorkspaceDraft(ws); localStorage.setItem("deepx:workspace", ws); }
        }).catch(() => {});
        // If the agent created a different seed (fallback from failed resume),
        // remap chatStores and activeSeed to the new seed.
        if (evtSeed !== listenerSeed) {
          chatStores.delete(listenerSeed);
          chatStores.set(evtSeed, chat);
          setActiveSeed(evtSeed);
        }
        refreshSessions();
        loadDashboardFromDisk(evtSeed, chat);
        break;
      }
      case "session_restored": if (p.seed) {
        const evtSeed = p.seed as string;
        chat.clearTurns();
        chat.handleSessionCreated(evtSeed);
        localStorage.setItem(LS_KEY, evtSeed);
        // Sync workspace from backend to draft
        invoke<string>("cmd_get_workspace", { seed: evtSeed }).then(ws => {
          chat.setWorkspace(ws);
          if (evtSeed === activeSeed()) { setWorkspaceDraft(ws); localStorage.setItem("deepx:workspace", ws); }
        }).catch(() => {});
        const turnsArr = p.turns as any[] | undefined;
        if (turnsArr && turnsArr.length > 0) {
          chat.loadTurnsFromRestore(turnsArr);
        } else if (!turnsArr || turnsArr.length === 0) {
          // Session exists but has no messages yet (freshly created).
          // This is normal — show empty chat, not an error.
          console.log("[App] session_restored with 0 turns — empty session");
        }
        chat.setHasMore(!!p.has_more);
        refreshSessions();
        // Load dashboard data directly from disk (no agent dependency)
        loadDashboardFromDisk(evtSeed, chat);
      } break;
      case "more_turns": if (p.turns) { chat.prependTurns(p.turns as any[]); chat.setHasMore(!!p.has_more); } break;
      case "dashboard": chat.handleDashboard(p); break;
      case "done": chat.handleDone(); chat.handleDone(); break;
      case "cancelled": chat.handleCancelled(); break;
      case "error": {
        const errMsg = (p.message ?? "Unknown error") as string;
        chat.handleError(errMsg);
        // Detect agent death: any message indicating the agent process is gone
        const isAgentDead = /(exited|died|broken.pipe|killed|connection.*lost|agent.*(dead|gone|stopped))/i.test(errMsg);
        if (isAgentDead) {
          toastCtrl.push(i18n.t().toast.agentLost, "error", true);
          // Auto-reconnect after agent death
          const seed = activeSeed();
          if (seed) {
            resumeSession(seed).then(() => {
              toastCtrl.push(i18n.t().toast.agentReconnected, "info");
            }).catch(() => {});
          }
        } else {
          toastCtrl.push(errMsg, "error");
        }
        break;
      }
      case "audit_record": chat.handleAuditRecord({ tool_name: (p.tool_name ?? "") as string, summary: (p.result_summary ?? "") as string, success: (p.success ?? false) as boolean, time: (p.time ?? "") as string, args: (p.args ?? "{}") as string }); break;
      case "compact_start": chat.handleCompactStart(p); break;
      case "compact_end": chat.handleCompactEnd(p); break;
      case "tool_notice": chat.handleToolNotice(p); break;
      case "permission_request": {
        setPermissionRequest({
          tool_call_id: (p.tool_call_id ?? "") as string,
          tool_name: (p.tool_name ?? "") as string,
          reason: (p.reason ?? "") as string,
          paths: (Array.isArray(p.paths) ? p.paths : []) as string[],
          category: (p.category ?? "") as string,
          level: (p.level ?? 4) as number,
        });
        break;
      }

      case "exec_progress": chat.handleExecProgress((p.tool_call_id ?? "") as string, (p.chunk ?? "") as string); break;
    }
  }

  async function refreshSessions() {
    try {
      const raw = await invoke<string>("cmd_list_sessions");
      const list: SessionMeta[] = JSON.parse(raw);
      list.sort((a, b) => Number(b.updated_at) - Number(a.updated_at));
      setSessions(list);
    } catch (e) { console.error(e); }
  }

  /** Load tasks + recent edits from disk, bypassing agent. */
  async function loadDashboardFromDisk(seed: string, chat: ChatStore) {
    try {
      const raw = await invoke<string>("cmd_get_dashboard_data", { seed });
      const data = JSON.parse(raw);
      if (data.tasks && data.tasks.length > 0) {
        chat.handleDashboard({ tasks: data.tasks, recent_edits: data.recent_edits });
      }
    } catch (e) { console.error("loadDashboardFromDisk:", e); }
  }

  async function resumeSession(seed: string) {
    console.log("[App] resumeSession called, seed:", seed);
    try {
      const existing = chatStores.get(seed);
      // If already open and fully initialized, just switch — don't clear or re-invoke
      if (existing && existing.sessionInfo.seed) {
        setActiveSeed(seed);
        setHasChosenSession(true);
        setView("chat");
        localStorage.setItem(LS_KEY, seed);
        // Restore per-session workspace
        try {
          const ws = await invoke<string>("cmd_get_workspace", { seed });
          existing.setWorkspace(ws);
          setWorkspaceDraft(ws);
          localStorage.setItem("deepx:workspace", ws);
        } catch (_) {}
        return;
      }
      const chat = await getOrCreateChatStore(seed);
      console.log("[App] invoking cmd_resume_session...");
      await invoke("cmd_resume_session", { seed });
      console.log("[App] cmd_resume_session returned");
      // Only commit UI state after successful backend call
      localStorage.setItem(LS_KEY, seed);
      setActiveSeed(seed);
      setHasChosenSession(true);
      setView("chat");
    } catch (e) {
      console.error("[App] resumeSession error:", e);
      // Reset to home on failure so user isn't stuck in blank chat view
      setHasChosenSession(false);
      setView("home");
    }
  }

  async function deleteSession(seed: string) {
    try {
      await invoke("cmd_delete_session", { seed });
      if (activeSeed() === seed) {
        const chat = chatStores.get(seed);
        if (chat) chat.clear();
        // Clean up event listener
        const unlisten = unlistenMap.get(seed);
        if (unlisten) { unlisten(); unlistenMap.delete(seed); }
        chatStores.delete(seed);
        localStorage.removeItem(LS_KEY);
        setActiveSeed("");
        setHasChosenSession(false);
      }
      await refreshSessions();
    } catch (e) { console.error(e); }
  }

  async function loadMoreTurns() {
    const seed = activeSeed();
    if (!seed) return;
    const chat = activeChat();
    if (!chat) return;
    const ts = chat.turns;
    if (ts.length === 0) return;
    const firstId = ts[0].turn_id;
    try { await invoke("cmd_load_more_turns", { seed, beforeTurnId: firstId }); } catch (e) { console.error(e); }
  }

  async function newSession() {
    try {
      const seed: string = await invoke("cmd_new_session");
      const chat = await getOrCreateChatStore(seed);
      chat.clear();
      localStorage.removeItem(LS_KEY);
      // Apply workspace draft
      const ws = workspaceDraft();
      if (ws) {
        chat.setWorkspace(ws);
        try { await invoke("cmd_set_workspace", { seed, path: ws }); } catch (e) { console.error(e); }
      }
      setActiveSeed(seed);
      setHasChosenSession(true);
      setView("chat");
      await refreshSessions();
    } catch (e) { console.error(e); }
  }

  /** Called from StartupView when user types a message without a session. */
  async function startNewSessionAndSend(text: string) {
    try {
      const seed: string = await invoke("cmd_new_session");
      const chat = await getOrCreateChatStore(seed);
      chat.clear();
      localStorage.removeItem(LS_KEY);
      // Apply workspace draft before sending the message
      const ws = workspaceDraft();
      if (ws) {
        chat.setWorkspace(ws);
        try { await invoke("cmd_set_workspace", { seed, path: ws }); } catch (e) { console.error(e); }
      }
      setActiveSeed(seed);
      setHasChosenSession(true);
      setView("chat");
      await refreshSessions();
      // Now send the message
      await invoke("cmd_send_message", { seed, text });
    } catch (e) { console.error(e); }
  }

  async function saveWorkspace(val: string) {
    setWorkspaceDraft(val);
    localStorage.setItem("deepx:workspace", val);
    const seed = activeSeed();
    const chat = activeChat();
    if (chat) chat.setWorkspace(val);
    if (!seed) return; // Persisted to localStorage; will apply when session is created.
    try { await invoke("cmd_set_workspace", { seed, path: val }); } catch (e) { console.error(e); }
  }

  async function browseWorkspace() {
    try {
      const selected = await open({ directory: true, multiple: false, title: t().session.workspace });
      if (selected && typeof selected === "string") {
        setWorkspaceDraft(selected);
        localStorage.setItem("deepx:workspace", selected);
        const seed = activeSeed();
        const chat = activeChat();
        if (chat) chat.setWorkspace(selected);
        if (seed) await invoke("cmd_set_workspace", { seed, path: selected });
      }
    } catch (e) { console.error(e); }
  }

  onMount(async () => {
    // ── Theme initialization ──
    const savedTheme = (localStorage.getItem(LS_THEME) ?? "system") as ThemeMode;
    setTheme(savedTheme);
    applyTheme(savedTheme);
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onSysThemeChange = () => {
      if ((localStorage.getItem(LS_THEME) ?? "system") === "system") {
        applyTheme("system");
      }
    };
    mq.addEventListener("change", onSysThemeChange);
    unlistenTheme = () => mq.removeEventListener("change", onSysThemeChange);

    // Load config
    try {
      const raw = await invoke<string>("cmd_load_config");
      const cfg = JSON.parse(raw);
      if (cfg.lang && (cfg.lang === "en" || cfg.lang === "zh")) {
        const cl = cfg.lang as Lang;
        i18n.setLang(cl);
        setConfigLang(cl);
        localStorage.setItem("deepx:lang", cl);
      }
    } catch (_) {}

    await refreshSessions();

    // Auto-resume saved session from last app close
    const savedSeed = localStorage.getItem(LS_KEY);
    if (savedSeed) {
      // Verify the session still exists in the list
      const exists = sessions().some((s) => s.seed === savedSeed);
      if (exists) {
        try {
          const ws = await invoke<string>("cmd_get_workspace", { seed: savedSeed });
          // Store workspace in the session's ChatStore once created
          const existingStore = chatStores.get(savedSeed);
          if (existingStore) existingStore.setWorkspace(ws);
          // Sync to draft if it's currently empty (session workspace takes priority)
          if (ws && !workspaceDraft()) { setWorkspaceDraft(ws); localStorage.setItem("deepx:workspace", ws); }
        } catch (_) {}
        // Resume the session — this spawns the agent and loads history
        resumeSession(savedSeed);
      } else {
        // Session was deleted externally — clear stale key
        localStorage.removeItem(LS_KEY);
      }
    }
  });

  onCleanup(() => {
    // Unregister all event listeners
    for (const [seed, unlisten] of unlistenMap) {
      try { unlisten(); } catch (_) {}
    }
    unlistenMap.clear();
    unlistenTheme?.();
    // Close all open sessions — await all to prevent lock contention
    // with the next page load's cmd_resume_session.
    const closePromises: Promise<void>[] = [];
    for (const seed of chatStores.keys()) {
      closePromises.push(
        invoke("cmd_close_session", { seed }).then(() => {}).catch(() => {})
      );
    }
    // Fire-and-forget but stored so cleanup runs before page fully unloads.
    // Tauri's on_window_close can await these if needed.
    Promise.allSettled(closePromises);
  });

  const t = () => i18n.t() ?? en;
  function handleSlashCommand(cmd: SlashCommand) {
    switch (cmd.id) {
      case "settings": setView("settings"); break;
      case "new": newSession(); break;
      case "compact": invoke("cmd_compact", { seed: activeSeed() }).catch(console.error); break;
      case "undo": {
        const chat = activeChat();
        if (chat) {
          const turns = chat.turns;
          if (turns.length > 0) chat.undoTurn(turns[turns.length - 1].turn_id);
        }
        break;
      }
    }
  }

  async function switchLang(l: Lang) { i18n.setLang(l); setConfigLang(l); localStorage.setItem("deepx:lang", l); try { await invoke("cmd_save_config", { apiKey: "", model: "", baseUrl: "", providerId: "", endpoint: "", maxTokens: 0, contextLimit: 0, reasoningEffort: "", lang: l, context7ApiKey: "", subagentModel: "", subagentBaseUrl: "", subagentApiKey: "", subagentMaxTokens: 0, subagentTimeoutSecs: 0, subagentDefaultTools: [] }); } catch (e) { console.error(e); } }
  function switchTheme(t: ThemeMode) { setTheme(t); localStorage.setItem(LS_THEME, t); applyTheme(t); }

  const isActive = (seed: string) => activeSeed() === seed;

  return (
    <I18nCtx.Provider value={{ t: i18n.t, lang: () => i18n.lang(), setLang: switchLang }}>
      <div class="app-container">
        <aside class="sidebar frost-panel">
          <div class="sidebar-brand"><span class="sidebar-logo">{">"}</span><span class="sidebar-title">{t().app.title}</span></div>
          <nav class="sidebar-nav">
            <button class={`sidebar-btn ${view() === "home" ? "active" : ""}`} onClick={() => setView("home")} title={t().nav.home}>
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 9l9-7 9 7v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/><polyline points="9 22 9 12 15 12 15 22"/></svg>
              <span>{t().nav.home}</span>
            </button>
            <button class={`sidebar-btn ${view() === "chat" ? "active" : ""}`} onClick={() => {
              setView("chat");
              if (!hasChosenSession() || !activeSeed()) {
                const list = sessions();
                if (list.length > 0) resumeSession(list[0].seed);
              }
            }} title={t().nav.chat}>
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>
              <span>{t().nav.chat}</span>
            </button>
            <button class={`sidebar-btn ${view() === "settings" ? "active" : ""}`} onClick={() => setView("settings")} title={t().nav.settings}>
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="3"/><path d="M12 1v2M12 21v2M4.22 4.22l1.42 1.42M18.36 18.36l1.42 1.42M1 12h2M21 12h2M4.22 19.78l1.42-1.42M18.36 5.64l1.42-1.42"/></svg>
              <span>{t().nav.settings}</span>
            </button>
          </nav>
          <div class="sidebar-section-label">{t().session.resume}</div>
          <button class="sidebar-new-session-btn" onClick={() => { newSession(); setHasChosenSession(true); }}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>
            <span>{t().session.new}</span>
          </button>
          <div class="sidebar-sessions">
            <For each={sessions()}>
              {(s) => (
                <button class={`sidebar-session-item ${isActive(s.seed) ? "active" : ""}`} onClick={() => resumeSession(s.seed)} title={s.last_summary || s.seed}>
                  <span class={`session-dot ${s.running ? "running" : ""} ${s.turso_backed ? "turso" : ""}`} title={s.turso_backed ? "SQLite" : "JSONL"} />
                  <span class="session-info">
                    <span class="session-summary">{s.last_summary || s.seed.substring(0, 8)}</span>
                    <span class="session-meta">{formatDate(Number(s.updated_at))} · {s.turn_count || s.message_count} {t().session.turns}</span>
                  </span>
                  <span
                    class="session-delete-btn"
                    onClick={(e) => { e.stopPropagation(); deleteSession(s.seed); }}
                    title={t().session.deleteHint}
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
          <div class="sidebar-workspace">
            <label class="sidebar-workspace-label">{t().session.workspace}</label>
            <div class="sidebar-workspace-row">
              <input
                class="sidebar-workspace-input"
                type="text"
                value={workspaceDraft()}
                placeholder={t().session.workspaceHint}
                onInput={(e) => { setWorkspaceDraft(e.currentTarget.value); const chat = activeChat(); if (chat) chat.setWorkspace(e.currentTarget.value); }}
                onChange={(e) => saveWorkspace(e.currentTarget.value)}
              />
              <button class="sidebar-workspace-browse" onClick={browseWorkspace} title={t().session.workspaceBrowse}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <path d="M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z"/>
                </svg>
              </button>
            </div>
          </div>
          <Show when={version()}>
            <button class="sidebar-version" onClick={() => setShowChangelog(true)} title="Changelog">
              v{version()}
            </button>
          </Show>
          <div
            class="sidebar-resize-handle"
            onMouseDown={(e) => {
              e.preventDefault();
              const startX = e.clientX;
              const startW = sidebarW();
              const handle = e.currentTarget as HTMLElement;
              handle.classList.add("active");
              const onMove = (ev: MouseEvent) => {
                const w = Math.max(160, Math.min(500, startW + ev.clientX - startX));
                setSidebarW(w);
                document.documentElement.style.setProperty("--sidebar-w", w + "px");
              };
              const onUp = () => {
                handle.classList.remove("active");
                localStorage.setItem("deepx:sidebar-w", String(sidebarW()));
                document.removeEventListener("mousemove", onMove);
                document.removeEventListener("mouseup", onUp);
              };
              document.addEventListener("mousemove", onMove);
              document.addEventListener("mouseup", onUp);
            }}
          />
        </aside>
        <main class="main-content">
          <Show when={chatStores.size > 0}>
          <div class="open-tabs">
            <For each={[...chatStores.keys()]}>
              {(seed) => (
                <button
                  class={`tab-btn ${seed === activeSeed() ? "active" : ""}`}
                  onClick={() => { setActiveSeed(seed); setHasChosenSession(true); setView("chat"); }}
                >
                  <span>{seed.substring(0, 8)}</span>
                  <span
                    class="tab-close"
                    onClick={(e) => {
                      e.stopPropagation();
                      invoke("cmd_close_session", { seed }).catch(console.error);
                      // Unregister event listener to prevent ghost updates
                      const unlisten = unlistenMap.get(seed);
                      if (unlisten) { unlisten(); unlistenMap.delete(seed); }
                      chatStores.delete(seed);
                      if (activeSeed() === seed) {
                        const remaining = [...chatStores.keys()];
                        setActiveSeed(remaining.length > 0 ? remaining[remaining.length - 1] : "");
                      }
                    }}
                  >×</span>
                </button>
              )}
            </For>
          </div>
          </Show>
          <Switch>
            <Match when={view() === "settings"}>
              <SettingsView lang={configLang} onLangChange={switchLang} theme={theme} onThemeChange={switchTheme} />
            </Match>
            <Match when={view() === "home"}>
              <div class="home-dashboard">
                <TokenChart refreshKey={refreshKey()} />
                <StartupView sessions={sessions()} onResume={resumeSession} onSend={startNewSessionAndSend} showHeatmap={true} />
              </div>
            </Match>
            <Match when={view() === "chat"}>
              <Show when={hasChosenSession() && activeSeed() && activeChat()}>
                <div class="chat-area">
                  <ChatView chat={activeChat()!} hasMore={activeChat()!.hasMore()} onLoadMore={loadMoreTurns} onSlashCommand={handleSlashCommand} />
                  <StatusPanel tasks={activeChat()!.tasks} recentEdits={activeChat()!.recentEdits} activityLog={activeChat()!.activityLog} seed={activeSeed()} loadActivityFromBackend={activeChat()!.loadActivityFromBackend} onTaskAction={(action, taskId, subject, desc) => activeChat()!.submitTaskAction(action, taskId, subject, desc)} />
                </div>
              </Show>
            </Match>
          </Switch>
        </main>
        
      </div>
      <ToastContainer ctrl={toastCtrl} />
      <Show when={permissionRequest()}>
        <PermissionDialog
          request={permissionRequest()!}
          seed={activeSeed()}
          onClose={() => setPermissionRequest(null)}
        />
      </Show>
      <Show when={showChangelog()}>
        <ChangelogModal onClose={() => setShowChangelog(false)} />
      </Show>
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
