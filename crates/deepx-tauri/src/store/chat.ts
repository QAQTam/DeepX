import { createStore, produce } from "solid-js/store";
import { createSignal } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { ToolCallDef, ToolResultDef, RoundBlock, RoundData, TurnData, TaskInfo, SessionMeta } from "@/lib/types";

// Re-export for other modules
export type { ToolCallDef, ToolResultDef, RoundBlock, TaskInfo, SessionMeta };
export interface Round extends RoundData {
  blocks: RoundBlock[];
}
export interface Turn {
  turn_id: string;
  user_text: string;
  rounds: Round[];
  status: "streaming" | "complete";
  stop_reason?: string;
  usage?: { input_tokens: number; output_tokens: number; total_tokens: number };
}
export interface SessionInfo { seed: string; model: string; context_tokens: number; context_limit: number; total_tokens: number; prompt_cache_hit: number; prompt_cache_miss: number; }
export interface ActivityEntry { tool_name: string; summary: string; success: boolean; time: number; }
export interface AskState { question: string; options: string[]; allow_custom: boolean; show: boolean; }

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
  const [askState, setAskState] = createSignal<AskState>({ question: "", options: [], allow_custom: true, show: false });
  const [isCompacting, setIsCompacting] = createSignal(false);

  const [compactResult, setCompactResult] = createSignal<string | null>(null);
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

  // ── RAF batching: coalesce rapid delta updates into a single setTurns per frame ──
  let rafPending = false;
  let pendingTurnId = "";
  let pendingRoundNum = 0;

  function flushDeltas() {
    if (!pendingTurnId) return;
    const tid = pendingTurnId;
    const rn = pendingRoundNum;
    pendingTurnId = "";
    pendingRoundNum = 0;
    setTurns((t) => t.turn_id === tid, "rounds", (r) => r.round_num === rn, produce((round: Round) => {
      round.thinking = streamBuffer.thinking;
      round.answer = streamBuffer.answer;
    }));
  }

  function ensureRound(turn_id: string, round_num: number) {
    const turn = turns.find((t) => t.turn_id === turn_id);
    if (!turn) return;
    const idx = turn.rounds.findIndex((r) => r.round_num === round_num);
    if (idx < 0) {
      setTurns((t) => t.turn_id === turn_id, "rounds", (r) => [...r, { round_num, thinking: "", answer: "", tool_calls: [], tool_results: [], blocks: [] } as Round]);
    }
  }

  let lastRoundNum = 0;

  function handleTurnStart(turn_id: string, user_text: string) {
    lastRoundNum = 0;
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
      resetStreamBuffer();
      lastRoundNum = round_num;
    }
    if (kind === "thinking") streamBuffer.thinking += delta; else if (kind === "answering") streamBuffer.answer += delta;
    ensureRound(turn_id, round_num);
    // RAF-batched: defer setTurns to next animation frame to coalesce rapid deltas
    pendingTurnId = turn_id;
    pendingRoundNum = round_num;
    if (!rafPending) {
      rafPending = true;
      requestAnimationFrame(() => {
        rafPending = false;
        flushDeltas();
      });
    }
  }

  // ── RAF batching for tool call previews ──
  let tcRafPending = false;
  let pendingTcMap = new Map<string, { id: string; name: string; argsSoFar: string }>();

  function flushToolCallPreviews(turn_id: string, round_num: number) {
    const batch = [...pendingTcMap.values()];
    pendingTcMap.clear();
    if (batch.length === 0) return;
    setTurns((t) => t.turn_id === turn_id, "rounds", (r) => r.round_num === round_num, produce((round: Round) => {
      for (const u of batch) {
        const existing = round.tool_calls.findIndex(tc => tc.id === u.id);
        if (existing >= 0) {
          round.tool_calls[existing].args_display = u.argsSoFar.slice(0, 100);
          round.tool_calls[existing].args_json = u.argsSoFar;
        } else {
          round.tool_calls.push({ id: u.id, name: u.name, args_display: u.argsSoFar.slice(0, 100), args_json: u.argsSoFar });
        }
      }
    }));
  }

  function handleToolCallPreview(turn_id: string, round_num: number, index: number, id: string, name: string, argsSoFar: string) {
    ensureRound(turn_id, round_num);
    pendingTcMap.set(id, { id, name, argsSoFar });
    if (!tcRafPending) {
      tcRafPending = true;
      requestAnimationFrame(() => {
        tcRafPending = false;
        flushToolCallPreviews(turn_id, round_num);
      });
    }
  }

  function handleRoundComplete(turn_id: string, round_num: number, thinking?: string, answer?: string, tool_calls?: ToolCallDef[], blocks?: RoundBlock[]) {
    flushDeltas(); // flush any pending streaming delta before replacing with final state
    ensureRound(turn_id, round_num);
    setTurns((t) => t.turn_id === turn_id, "rounds", (r) => r.round_num === round_num, produce((round: Round) => {
      if (thinking) round.thinking = thinking; if (answer) round.answer = answer; if (tool_calls) round.tool_calls = tool_calls; if (blocks) round.blocks = blocks;
    }));
  }

  // ── RAF batching for exec progress (tool output streaming) ──
  let execRafPending = false;
  let pendingExecChunks = new Map<string, string>(); // tool_call_id → accumulated output

  function flushExecProgress() {
    const batch = [...pendingExecChunks.entries()];
    pendingExecChunks.clear();
    if (batch.length === 0) return;
    setTurns(produce((ts) => {
      const turn = ts[ts.length - 1];
      if (!turn || turn.status !== "streaming") return;
      const round = turn.rounds[turn.rounds.length - 1];
      if (!round) return;
      for (const [streamKey, output] of batch) {
        const existing = round.tool_results.findIndex(tr => tr.tool_call_id === streamKey);
        if (existing >= 0) {
          round.tool_results[existing].output += output;
        } else {
          round.tool_results.push({ tool_call_id: streamKey, output, success: true });
        }
      }
    }));
  }

  function handleExecProgress(tool_call_id: string, chunk: string) {
    // Guard: ignore if the last turn is already complete (prevents bleed into next round)
    const lastTurn = turns[turns.length - 1];
    if (!lastTurn || lastTurn.status !== "streaming") return;

    const prev = pendingExecChunks.get(tool_call_id) || "";
    pendingExecChunks.set(tool_call_id, prev + chunk);
    if (!execRafPending) {
      execRafPending = true;
      requestAnimationFrame(() => {
        execRafPending = false;
        flushExecProgress();
      });
    }
  }

  function handleToolResults(turn_id: string, round_num: number, results: ToolResultDef[]) {
    flushExecProgress(); // flush any pending exec progress before replacing with final results
    ensureRound(turn_id, round_num);
    setTurns((t) => t.turn_id === turn_id, "rounds", (r) => r.round_num === round_num, produce((round: Round) => {
      // Remove streaming placeholders matching the final result IDs
      const finalIds = new Set(results.map(r => r.tool_call_id));
      round.tool_results = round.tool_results.filter(tr => !finalIds.has(tr.tool_call_id) || tr.success === undefined);
      // Append final results
      round.tool_results.push(...results);
    }));
    for (const r of results) {
      if (r.success && r.output.startsWith("[USER_QUERY] ")) {
        try {
          const json = JSON.parse(r.output.slice(13));
          setAskState({ question: json.question || "", options: json.options || [], allow_custom: json.allow_custom !== false, show: true });
        } catch {}
      }
    }
  }

  function handleTurnEnd(turn_id: string, data: Record<string, unknown>) {
    flushDeltas(); flushToolCallPreviews(turn_id, lastRoundNum); flushExecProgress(); // flush all pending state before marking complete
    setIsStreaming(false); setInputDisabled(false); resetStreamBuffer(); lastRoundNum = 0;
    setTurns((t) => t.turn_id === turn_id, produce((turn) => { turn.status = "complete"; turn.stop_reason = data.stop_reason as string | undefined; if (data.usage) turn.usage = data.usage as Turn["usage"]; }));
    const u = data.usage as Record<string, unknown> | undefined;
    if (u) {
      setSessionInfo(produce((s) => {
        if (u.prompt_tokens != null) s.context_tokens = u.prompt_tokens as number;
        if (u.total_tokens != null) s.total_tokens = (u.total_tokens as number);
        if (u.prompt_cache_hit_tokens != null) s.prompt_cache_hit = u.prompt_cache_hit_tokens as number;
        if (u.prompt_cache_miss_tokens != null) s.prompt_cache_miss = u.prompt_cache_miss_tokens as number;
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
    if (data.tasks) setTasks(data.tasks as TaskInfo[]);
    if (data.recent_edits) setRecentEdits(data.recent_edits as string[]);
  }

  function handleAuditRecord(entry: ActivityEntry) {
    setActivityLog((prev) => {
      const next = [entry, ...prev];
      return next.length > 50 ? next.slice(0, 50) : next;
    });
  }

  function handleCancelled() {
    setIsStreaming(false); setInputDisabled(false); resetStreamBuffer(); lastRoundNum = 0;
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
    setTurns([]); setError(null); setTasks([]); setRecentEdits([]); setActivityLog([]);
    resetStreamBuffer(); setIsStreaming(false); setInputDisabled(false); lastRoundNum = 0;
    setSessionInfo({ seed: "", model: "", context_tokens: 0, context_limit: 0, total_tokens: 0, prompt_cache_hit: 0, prompt_cache_miss: 0 });
  }

  function clearTurns() {
    cacheCurrentStatus();
    setTurns([]); resetStreamBuffer(); lastRoundNum = 0;
    setError(null);
  }

  function handleCompactStart(data: Record<string, unknown>) {
    setIsCompacting(true);
    setCompactResult(null);
  }

  function handleCompactEnd(data: Record<string, unknown>) {
    setIsCompacting(false);
    setCompactResult(`Compacted ${data.turns_compacted} turns`);
  }

  function handleToolNotice(data: Record<string, unknown>) {
    // Tool notices are informational; just log for now
  }

  async function undoTurn(turn_id: string) {
    try {
      await invoke("cmd_undo_turn", { seed, turnId: turn_id });
    } catch (e) { console.error(e); }
    setTurns((prev) => prev.filter(t => t.turn_id !== turn_id));
  }

  function handleSessionCreated(evtSeed: string) {
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
      } catch (e) { console.error(e); }
    }
  }

  async function submitAskAnswer(answer: string) {
    setAskState({ question: "", options: [], allow_custom: true, show: false });
    try {
      await invoke("cmd_send_message", { seed, text: answer });
    } catch (e) { console.error(e); }
  }

  function dismissAsk() { setAskState({ question: "", options: [], allow_custom: true, show: false }); }

  function loadSessionFromData(snapshot: { turns: Turn[]; info: SessionInfo }) {
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

  return { turns, sessionInfo, isStreaming, inputDisabled, hasMore, setHasMore, workspace, setWorkspace, error, restoreText, tasks, recentEdits, activityLog, askState, submitAskAnswer, dismissAsk, submitTaskAction, isCompacting, compactResult, handleCompactStart, handleCompactEnd, handleToolNotice, handleTurnStart, handleRoundDelta, handleToolCallPreview, handleRoundComplete, handleToolResults, handleExecProgress, handleTurnEnd, handleSessionCreated, handleDashboard, handleAuditRecord, handleCancelled, handleDone, handleError, clearError, clear, clearTurns, undoTurn, loadSessionFromData, loadTurnsFromRestore, prependTurns };
}
