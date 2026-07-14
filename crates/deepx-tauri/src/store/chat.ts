import { createStore, produce } from "solid-js/store";
import { createSignal, createEffect } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { ToolCallDef, ToolResultDef, RoundBlock, RoundData, TurnData, TaskInfo, SessionMeta, AskMode, AskQuestion, AskAnswer, SkillInfo } from "@/lib/types";
import type { MetricPoint } from "@/components/StreamMetricsChart";

// Re-export for other modules
export type { ToolCallDef, ToolResultDef, RoundBlock, TaskInfo, SessionMeta, SkillInfo };
export interface Round extends RoundData {
  blocks: RoundBlock[];
  thinking_ms?: number;
}
export interface Turn {
  turn_id: string;
  user_text: string;
  rounds: Round[];
  status: "streaming" | "complete";
  stop_reason?: string;
  usage?: { input_tokens: number; output_tokens: number; total_tokens: number; completion_tokens?: number };
  metrics?: { thinking_ms: number; output_ms: number; tokens_per_sec: number; answer_tokens?: number };
}
export interface SessionInfo { seed: string; model: string; context_tokens: number; context_limit: number; total_tokens: number; prompt_cache_hit: number; prompt_cache_miss: number; }
export interface ActivityEntry { tool_name: string; summary: string; success: boolean; time: string; args: string; }
export interface AskState {
  askId: string;
  mode: AskMode;
  questions: AskQuestion[];
  show: boolean;
}

export function createChatStore(seed: string) {
  const [turns, setTurns] = createStore<Turn[]>([]);
  const [sessionInfo, setSessionInfo] = createStore<SessionInfo>({ seed: "", model: "", context_tokens: 0, context_limit: 0, total_tokens: 0, prompt_cache_hit: 0, prompt_cache_miss: 0 });
  const [isStreaming, setIsStreaming] = createSignal(false);
  const [inputDisabled, setInputDisabled] = createSignal(false);
  const [hasMore, setHasMore] = createSignal(false);
  const [workspace, setWorkspace] = createSignal("");

  // Debug hook: inject mock data from browser console
  if (typeof window !== "undefined") {
    (window as any).__deepxDebugInject = (mockTurns: Turn[]) => {
      setTurns(mockTurns as any);
      setSessionInfo({ seed: "debug", model: "mock", context_tokens: 0, context_limit: 0, total_tokens: 0, prompt_cache_hit: 0, prompt_cache_miss: 0 });
    };
  }
  const [error, setError] = createSignal<string | null>(null);
  const [restoreText, setRestoreText] = createSignal<string | null>(null);
  const [tasks, setTasks] = createSignal<TaskInfo[]>([]);
  const [recentEdits, setRecentEdits] = createSignal<string[]>([]);
  const [activityLog, setActivityLog] = createSignal<ActivityEntry[]>([]);
  const [skillCatalog, setSkillCatalog] = createSignal<SkillInfo[]>([]);
  const [activeSkillNames, setActiveSkillNames] = createSignal<string[]>([]);
  const [askState, setAskState] = createSignal<AskState>({ askId: "", mode: "single" as AskMode, questions: [], show: false });
  let askLock: string | null = null;
  let askPending = false;
  const askQueue: AskState[] = [];
  const [isCompacting, setIsCompacting] = createSignal(false);

  const [compactResult, setCompactResult] = createSignal<number | null>(null);
  const [compactText, setCompactText] = createSignal("");
  const [metricHistory, setMetricHistory] = createSignal<MetricPoint[]>([]);
  // Periodic metric sampling during streaming (fills chart even when API doesn't send usage)
  let samplingInterval: ReturnType<typeof setInterval> | null = null;
  createEffect(() => {
    const streaming = isStreaming();
    if (streaming && !samplingInterval) {
      // Push initial baseline point so chart has an origin
      setMetricHistory((prev: MetricPoint[]) => {
        if (prev.length === 0) {
          return [{ ts: Date.now(), context_tokens: 0, cache_hit: 0, cache_miss: 0 }];
        }
        return prev;
      });
      samplingInterval =
      samplingInterval = setInterval(() => {
        setMetricHistory((prev: MetricPoint[]) => {
          const last = prev[prev.length - 1];
          const now = { ts: Date.now(), context_tokens: sessionInfo.context_tokens, cache_hit: sessionInfo.prompt_cache_hit, cache_miss: sessionInfo.prompt_cache_miss };
          // Always push to track elapsed time on chart
          const next = [...prev, now];
          return next.length > 120 ? next.slice(-120) : next;
        });
      }, 2000);
    } else if (!streaming && samplingInterval) {
      clearInterval(samplingInterval);
      samplingInterval = null;
    }
  });
  let streamBuffer = { thinking: "", answer: "" };

  // ── Per-session status cache ──
  type SessionStatus = { tasks: TaskInfo[]; edits: string[]; activity: ActivityEntry[] };
  const sessionStatusCache = new Map<string, SessionStatus>();

  function cacheCurrentStatus() {
    const seed = sessionInfo.seed;
    if (!seed) return;
    sessionStatusCache.set(seed, {
      tasks: [...tasks()],
      edits: [...recentEdits()],
      activity: [...activityLog()],
    });
  }

  function loadCachedStatus(seed: string) {
    const cached = sessionStatusCache.get(seed);
    if (cached) {
      setTasks(cached.tasks);
      setRecentEdits(cached.edits);
      setActivityLog(cached.activity);
    } else {
      setTasks([]);
      setRecentEdits([]);
      setActivityLog([]);
    }
  }

  function resetStreamBuffer() { streamBuffer = { thinking: "", answer: "" }; }

  // ── Direct render (no RAF batching — SolidJS granular updates are cheap) ──
  function flushDeltas(turn_id: string, round_num: number) {
    setTurns((t) => t.turn_id === turn_id, "rounds", (r) => r.round_num === round_num, produce((round: Round) => {
      round.thinking = streamBuffer.thinking;
      round.answer = streamBuffer.answer;
    }));
  }

  function ensureRound(turn_id: string, round_num: number) {
    const turn = turns.find((t) => t.turn_id === turn_id);
    if (!turn) return;
    const idx = turn.rounds.findIndex((r) => r.round_num === round_num);
    if (idx < 0) {
      setTurns((t) => t.turn_id === turn_id, "rounds", (r) => [...r, { round_num, is_final: false, thinking: "", answer: "", tool_calls: [], tool_results: [], blocks: [] } as Round]);
    }
  }

  let lastRoundNum = 0;
  let turnStartedAt = 0;
  let roundThinkingStart = 0;   // when current round's thinking started
  let roundAnswerStart = 0;     // when current round's answering started
  let cumulativeAnswerMs = 0;   // total time spent in "answering" phases

  function handleTurnStart(turn_id: string, user_text: string) {
    lastRoundNum = 0;
    turnStartedAt = Date.now(); roundThinkingStart = 0; roundAnswerStart = 0; cumulativeAnswerMs = 0;
    resetStreamBuffer();
    setIsStreaming(true); setInputDisabled(true); setError(null);
    // Add new turn at end — but guard against duplicate re-emission (resume race)
    const already = turns[turns.length - 1];
    if (already && already.turn_id === turn_id) return;
    setTurns((prev) => [...prev, { turn_id, user_text, rounds: [], status: "streaming" } as Turn]);
  }

  function handleRoundDelta(turn_id: string, round_num: number, kind: string, delta: string) {
    // Reset stream buffer when entering a new round (e.g. after tool calls)
    if (round_num !== lastRoundNum) {
      // Close previous round's timing before resetting
      if (roundAnswerStart > 0) {
        cumulativeAnswerMs += Date.now() - roundAnswerStart;
      }
      resetStreamBuffer();
      lastRoundNum = round_num;
      roundThinkingStart = 0;
      roundAnswerStart = 0;
    }
    if (kind === "thinking") {
      streamBuffer.thinking += delta;
      if (!roundThinkingStart) roundThinkingStart = Date.now();
    } else if (kind === "answering") {
      streamBuffer.answer += delta;
      if (!roundAnswerStart) roundAnswerStart = Date.now();
    }
    ensureRound(turn_id, round_num);
    flushDeltas(turn_id, round_num);
  }

  function handleToolCallPreview(turn_id: string, round_num: number, index: number, id: string, name: string, argsSoFar: string) {
    ensureRound(turn_id, round_num);
    setTurns((t) => t.turn_id === turn_id, "rounds", (r) => r.round_num === round_num, produce((round: Round) => {
      const existing = round.tool_calls.findIndex(tc => tc.id === id);
      if (existing >= 0) {
        round.tool_calls[existing].args_display = argsSoFar.slice(0, 100);
        round.tool_calls[existing].args_json = argsSoFar;
      } else {
        round.tool_calls.push({ id, name, args_display: argsSoFar.slice(0, 100), args_json: argsSoFar });
      }
    }));
  }

  function handleRoundComplete(turn_id: string, round_num: number, thinking?: string, answer?: string, tool_calls?: ToolCallDef[], blocks?: RoundBlock[], isFinal = false) {
    flushDeltas(turn_id, round_num); // flush any pending streaming delta before replacing with final state
    // Compute thinking elapsed for this round
    const thinkMs = roundThinkingStart ? Date.now() - roundThinkingStart : 0;
    ensureRound(turn_id, round_num);
    setTurns((t) => t.turn_id === turn_id, "rounds", (r) => r.round_num === round_num, produce((round: Round) => {
      if (thinking) round.thinking = thinking; if (answer) round.answer = answer; if (tool_calls) round.tool_calls = tool_calls; if (blocks) round.blocks = blocks; round.is_final = isFinal;
      if (thinkMs > 0) round.thinking_ms = thinkMs;
    }));
  }

  // ── Direct render for exec progress (tool output streaming) ──
  function handleExecProgress(tool_call_id: string, stream: "stdout" | "stderr", _seq: number, chunk: string) {
    // Guard: ignore if the last turn is already complete (prevents bleed into next round)
    const lastTurn = turns[turns.length - 1];
    if (!lastTurn || lastTurn.status !== "streaming") return;

    setTurns(produce((ts) => {
      const turn = ts[ts.length - 1];
      if (!turn || turn.status !== "streaming") return;
      const round = turn.rounds[turn.rounds.length - 1];
      if (!round) return;
      const streamingId = `${tool_call_id}_stream`;
      const existing = round.tool_results.findIndex(tr => tr.tool_call_id === streamingId);
      if (existing >= 0) {
        const result = round.tool_results[existing];
        result.output += stream === "stderr" ? `\n[stderr]\n${chunk}` : chunk;
      } else {
        round.tool_results.push({ tool_call_id: streamingId, output: stream === "stderr" ? `[stderr]\n${chunk}` : chunk, success: true });
      }
    }));
  }

  function handleToolResults(turn_id: string, round_num: number, results: ToolResultDef[]) {
    ensureRound(turn_id, round_num);
    setTurns((t) => t.turn_id === turn_id, "rounds", (r) => r.round_num === round_num, produce((round: Round) => {
      // Remove streaming placeholders matching the final result IDs
      const finalIds = new Set(results.map(r => r.tool_call_id));
      round.tool_results = round.tool_results.filter(tr =>
        (!finalIds.has(tr.tool_call_id) && !finalIds.has(tr.tool_call_id.replace(/_stream$/, ""))) || tr.success === undefined
      );
      // Append final results
      round.tool_results.push(...results);
    }));
  }

  function handleTurnEnd(turn_id: string, data: Record<string, unknown>) {
    flushDeltas(turn_id, lastRoundNum); // flush final streaming state
    setIsStreaming(false); setInputDisabled(false); resetStreamBuffer(); lastRoundNum = 0;
    const now = Date.now();
    // Close current round's answer timing
    if (roundAnswerStart > 0) {
      cumulativeAnswerMs += now - roundAnswerStart;
      roundAnswerStart = 0;
    }
    // Total thinking = sum of per-round thinking_ms (set in handleRoundComplete)
    setTurns((t) => t.turn_id === turn_id, produce((turn) => {
      turn.status = "complete";
      turn.stop_reason = data.stop_reason as string | undefined;
      if (data.usage) {
        turn.usage = data.usage as Turn["usage"];
        const cTok = (data.usage as Record<string, unknown>).completion_tokens as number | undefined;
        const rTok = (data.usage as Record<string, unknown>).reasoning_tokens as number | undefined;
        if (cTok) turn.usage!.completion_tokens = cTok;
        // answer tokens = total completion − reasoning (thinking) tokens
        const answerTok = cTok ? (cTok - (rTok || 0)) : 0;
        const tps = answerTok && cumulativeAnswerMs > 0 ? Math.round(answerTok / (cumulativeAnswerMs / 1000)) : 0;
        // Aggregate thinking across rounds
        const totalThinking = turn.rounds.reduce((sum, r) => sum + (r.thinking_ms || 0), 0);
        turn.metrics = { thinking_ms: totalThinking, output_ms: cumulativeAnswerMs, tokens_per_sec: tps, answer_tokens: answerTok };
      }
    }));
    const u = data.usage as Record<string, unknown> | undefined;
    if (u) {
      setSessionInfo(produce((s: SessionInfo) => {
        if (u.prompt_tokens != null && (u.prompt_tokens as number) > 0) s.context_tokens = u.prompt_tokens as number;
        if (u.completion_tokens != null) s.context_tokens = Math.max(s.context_tokens, (u.completion_tokens ?? 0) as number);
        if (u.total_tokens != null) s.total_tokens = (u.total_tokens as number);
        if (u.prompt_cache_hit_tokens != null) s.prompt_cache_hit = u.prompt_cache_hit_tokens as number;
        if (u.prompt_cache_miss_tokens != null) s.prompt_cache_miss = u.prompt_cache_miss_tokens as number;
        setMetricHistory((prev: MetricPoint[]) => {
          const next = [...prev, { ts: Date.now(), context_tokens: (u.prompt_tokens ?? 0) as number, cache_hit: (u.prompt_cache_hit_tokens ?? 0) as number, cache_miss: (u.prompt_cache_miss_tokens ?? 0) as number }];
          return next.length > 120 ? next.slice(-120) : next;
        });
      }));
    }
  }

  function handleDashboard(data: Record<string, unknown>) {
    setSessionInfo(produce((s) => {
      if (data.session_seed) s.seed = data.session_seed as string;
      if (data.model) s.model = data.model as string;
      if (data.context_limit != null) s.context_limit = data.context_limit as number;
      if (data.usage != null) {
        const u = data.usage as Record<string, unknown>;
        if (u.prompt_tokens != null) s.context_tokens = u.prompt_tokens as number;
        if (u.total_tokens != null) s.total_tokens = u.total_tokens as number;
        if (u.prompt_cache_hit_tokens != null) s.prompt_cache_hit = u.prompt_cache_hit_tokens as number;
        if (u.prompt_cache_miss_tokens != null) s.prompt_cache_miss = u.prompt_cache_miss_tokens as number;
      }
    }));
    if (data.usage) {
      const u = data.usage as Record<string, unknown>;
      setMetricHistory((prev: MetricPoint[]) => {
        const next = [...prev, { ts: Date.now(), context_tokens: (u.prompt_tokens ?? 0) as number, cache_hit: (u.prompt_cache_hit_tokens ?? 0) as number, cache_miss: (u.prompt_cache_miss_tokens ?? 0) as number }];
        return next.length > 120 ? next.slice(-120) : next;
      });
    }
    if (Array.isArray(data.tasks)) setTasks(data.tasks as TaskInfo[]);
    if (Array.isArray(data.recent_edits)) setRecentEdits(data.recent_edits as string[]);
  }

  function handleSkillsChanged(data: Record<string, unknown>) {
    if (data.available && Array.isArray(data.available)) {
      setSkillCatalog(data.available as SkillInfo[]);
    }
    if (data.active && Array.isArray(data.active)) {
      setActiveSkillNames(data.active as string[]);
    }
  }

  function handleAuditRecord(entry: ActivityEntry) {
    setActivityLog((prev) => {
      const next = [entry, ...prev];
      return next.length > 50 ? next.slice(0, 50) : next;
    });
  }

  async function loadActivityFromBackend() {
    try {
      const raw = await invoke<string>("cmd_get_activity", { seed });
      const list = JSON.parse(raw) as ActivityEntry[];
      setActivityLog(list);
    } catch (e) {
      console.error("loadActivity:", e);
    }
  }

  function handleCancelled() {
    resetAskLifecycle();
    setIsStreaming(false); setInputDisabled(false); resetStreamBuffer(); lastRoundNum = 0;
    turnStartedAt = 0; roundThinkingStart = 0; roundAnswerStart = 0; cumulativeAnswerMs = 0;
  }

  function handleDone() {
    setIsStreaming(false);
    setInputDisabled(false);
  }

  function handleError(msg: string) {
    setError(msg); setIsStreaming(false); setInputDisabled(false);
    const lastTurn = turns[turns.length - 1];
    if (lastTurn && lastTurn.status === "streaming") setRestoreText(lastTurn.user_text);
  }
  function clearError() { setError(null); }

  function clear() {
    resetAskLifecycle();
    setTurns([]); setError(null); setTasks([]); setRecentEdits([]); setActivityLog([]);
    resetStreamBuffer(); setIsStreaming(false); setInputDisabled(false); lastRoundNum = 0;
    turnStartedAt = 0; roundThinkingStart = 0; roundAnswerStart = 0; cumulativeAnswerMs = 0;
    setSessionInfo({ seed: "", model: "", context_tokens: 0, context_limit: 0, total_tokens: 0, prompt_cache_hit: 0, prompt_cache_miss: 0 });
  }

  function clearTurns() {
    resetAskLifecycle();
    cacheCurrentStatus();
    setTurns([]); resetStreamBuffer(); lastRoundNum = 0;
    turnStartedAt = 0; roundThinkingStart = 0; roundAnswerStart = 0; cumulativeAnswerMs = 0;
    setError(null);
  }

  function handleCompactStart(data: Record<string, unknown>) {
    setIsCompacting(true);
    setCompactResult(null);
    setCompactText("");
  }

  function handleCompactEnd(data: Record<string, unknown>) {
    setIsCompacting(false);
    setCompactText("");
    setCompactResult(data.turns_compacted as number);
    setTimeout(() => setCompactResult(null), 4000);
  }

  function handleCompactDelta(data: Record<string, unknown>) {
    setCompactText((prev) => prev + (data.delta as string));
  }

  function handleToolNotice(data: Record<string, unknown>) {
    // Tool notices are informational; just log for now
  }

  async function undoTurn(turn_id: string) {
    try {
      await invoke("cmd_undo_turn", { seed, turnId: turn_id });
    } catch (e) {
      console.error(e);
      return;
    }
    resetAskLifecycle();
    setTurns((prev) => prev.filter(t => t.turn_id !== turn_id));
  }

  function handleSessionCreated(evtSeed: string) {
    resetAskLifecycle();
    cacheCurrentStatus();
    setSessionInfo(produce((s) => { s.seed = evtSeed; }));
    loadCachedStatus(evtSeed);
  }

  async function submitTaskAction(action: "cancel" | "delete" | "ask", taskId: string, subject: string, _description: string) {
    if (action === "ask") {
      try {
        await invoke("cmd_send_message", { seed, text: `Look at ${taskId}: ${subject}. Explain the implementation plan and current status in detail.` });
      } catch (e) { console.error(e); }
    } else {
      const num = parseInt(taskId.replace("T", ""), 10);
      if (isNaN(num)) return;
      try {
        await invoke("cmd_task_action", { seed, action, taskId: num });
        const raw = await invoke<string>("cmd_get_dashboard_data", { seed });
        handleDashboard(JSON.parse(raw));
      } catch (e) { console.error(e); }
    }
  }

  // ── Ask dialog helpers ──

  function resetAskLifecycle() {
    askLock = null;
    askPending = false;
    askQueue.length = 0;
    setAskState({ askId: "", mode: "single" as AskMode, questions: [], show: false });
  }

  function showAskDialog(json: Record<string, unknown>) {
    const askId = typeof json.ask_id === "string" ? json.ask_id.trim() : "";
    if (!askId) return;
    const mode: AskMode = (json.mode as AskMode) || "single";
    const questions: AskQuestion[] = Array.isArray(json.questions)
      ? json.questions as unknown as AskQuestion[]
      : [{ id: "q1", question: (json.question as string) || "", options: (json.options as string[]) || [], allow_custom: (json.allow_custom as boolean) !== false }];
    const next = { askId, mode, questions, show: true };
    if (askLock !== null) {
      if (askLock !== askId && !askQueue.some(item => item.askId === askId)) askQueue.push(next);
      return;
    }
    askLock = askId;
    askPending = false;
    setAskState(next);
  }

  function showNextAsk() {
    const next = askQueue.shift();
    if (next) {
      askLock = next.askId;
      askPending = false;
      setAskState(next);
    }
  }

  function handleAskResolved(askId: string) {
    if (!askLock || askLock !== askId) return;
    askLock = null;
    askPending = false;
    setAskState({ askId: "", mode: "single" as AskMode, questions: [], show: false });
    showNextAsk();
  }

  function handleAskRejected(askId: string, message: string) {
    if (askLock !== askId) return;
    askPending = false;
    setError(message);
  }

  async function submitAskAnswer(answers: AskAnswer[]) {
    const askId = askLock;
    if (!askId || askPending) return;
    askPending = true;
    try {
      await invoke("cmd_ask_response", { seed, askId, answers });
    } catch (e) { askPending = false; console.error(e); }
  }

  async function dismissAsk() {
    const askId = askLock;
    if (!askId || askPending) return;
    askPending = true;
    try {
      await invoke("cmd_ask_dismiss", { seed, askId });
    } catch (e) { askPending = false; console.error(e); }
  }

  function loadSessionFromData(snapshot: { turns: Turn[]; info: SessionInfo }) {
    resetAskLifecycle();
    setTurns(snapshot.turns);
    setSessionInfo(snapshot.info);
  }

  // Simplified: wire format is now snake_case; blocks are derived UI-side
  function loadTurnsFromRestore(turnsData: TurnData[]) {
    setTurns(turnsData.map((td) => ({
      turn_id: td.turn_id,
      user_text: td.user_text,
      rounds: td.rounds.map((rd) => {
        const blocks: RoundBlock[] = [];
        if (rd.thinking) blocks.push({ type: "reasoning", content: rd.thinking });
        if (rd.answer) blocks.push({ type: "text", content: rd.answer });
        for (const tc of rd.tool_calls) blocks.push({ type: "tool", card: tc });
        return { ...rd, blocks } as Round;
      }),
      status: "complete" as const,
    })));
  }

  // Simplified: wire format is now snake_case; blocks are derived UI-side
  function prependTurns(turnsData: TurnData[]) {
    const loaded: Turn[] = turnsData.map((td) => ({
      turn_id: td.turn_id,
      user_text: td.user_text,
      rounds: td.rounds.map((rd) => {
        const blocks: RoundBlock[] = [];
        if (rd.thinking) blocks.push({ type: "reasoning", content: rd.thinking });
        if (rd.answer) blocks.push({ type: "text", content: rd.answer });
        for (const tc of rd.tool_calls) blocks.push({ type: "tool", card: tc });
        return { ...rd, blocks } as Round;
      }),
      status: "complete" as const,
    }));
    setTurns(produce((prev) => { prev.unshift(...loaded); }));
  }

  return { turns, sessionInfo, isStreaming, inputDisabled, hasMore, setHasMore, workspace, setWorkspace, error, restoreText, tasks, recentEdits, activityLog, skillCatalog, activeSkillNames, loadActivityFromBackend, askState, showAskDialog, handleAskResolved, handleAskRejected, submitAskAnswer, dismissAsk, submitTaskAction, isCompacting, compactResult, compactText, metricHistory, handleCompactStart, handleCompactEnd, handleCompactDelta, handleToolNotice, handleTurnStart, handleRoundDelta, handleToolCallPreview, handleRoundComplete, handleToolResults, handleExecProgress, handleTurnEnd, handleSessionCreated, handleDashboard, handleSkillsChanged, handleAuditRecord, handleCancelled, handleDone, handleError, clearError, clear, clearTurns, undoTurn, loadSessionFromData, loadTurnsFromRestore, prependTurns };
}
