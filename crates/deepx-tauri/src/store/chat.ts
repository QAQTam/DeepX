import { createStore, produce } from "solid-js/store";
import { createSignal } from "solid-js";
import { invoke } from "@tauri-apps/api/core";

export interface ToolCallDef { id: string; name: string; args_display: string; args_json: string; }
export interface ToolResultDef { tool_call_id: string; output: string; success: boolean; }
export interface RoundBlock { type: "reasoning" | "text" | "tool"; content?: string; card?: ToolCallDef; }
export interface Round { roundNum: number; thinking?: string; answer?: string; blocks: RoundBlock[]; toolCalls: ToolCallDef[]; toolResults: ToolResultDef[]; }
export interface Turn { turnId: string; userText: string; rounds: Round[]; status: "streaming" | "complete"; stopReason?: string; usage?: { input_tokens: number; output_tokens: number; total_tokens: number }; }
export interface SessionInfo { seed: string; model: string; contextTokens: number; contextLimit: number; totalTokens: number; promptCacheHit: number; promptCacheMiss: number; }
export interface SessionMeta { seed: string; model: string; created_at: number; updated_at: number; message_count: number; turn_count: number; last_summary: string; }
export interface TaskInfo { id: string; subject: string; description: string; status: string; }
export interface ActivityEntry { tool_name: string; summary: string; success: boolean; time: number; }
export interface AskState { question: string; options: string[]; allow_custom: boolean; show: boolean; }
export interface CodeDelta {
 lines_added: number;
 lines_removed: number;
 files_created: number;
 files_deleted: number;
 file?: string;
}

export interface CodeDelta {
  lines_added: number;
  lines_removed: number;
  files_created: number;
  files_deleted: number;
  file?: string;
}

export function createChatStore(seed: string) {
  const [turns, setTurns] = createStore<Turn[]>([]);
  const [sessionInfo, setSessionInfo] = createStore<SessionInfo>({ seed: "", model: "", contextTokens: 0, contextLimit: 0, totalTokens: 0, promptCacheHit: 0, promptCacheMiss: 0 });
  const [isStreaming, setIsStreaming] = createSignal(false);
  const [inputDisabled, setInputDisabled] = createSignal(false);
  const [hasMore, setHasMore] = createSignal(false);
  const [workspace, setWorkspace] = createSignal("");
  const [codeDeltas, setCodeDeltas] = createSignal<CodeDelta[]>([]);

  // Debug hook: inject mock data from browser console
  if (typeof window !== "undefined") {
    (window as any).__deepxDebugInject = (mockTurns: Turn[]) => {
      setTurns(mockTurns as any);
      setSessionInfo({ seed: "debug", model: "mock", contextTokens: 0, contextLimit: 0, totalTokens: 0, promptCacheHit: 0, promptCacheMiss: 0 });
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
    setTurns((t) => t.turnId === tid, "rounds", (r) => r.roundNum === rn, produce((round) => {
      round.thinking = streamBuffer.thinking;
      round.answer = streamBuffer.answer;
    }));
  }

  function ensureRound(turnId: string, roundNum: number) {
    const turn = turns.find((t) => t.turnId === turnId);
    if (!turn) return;
    if (turn.rounds.find((r) => r.roundNum === roundNum)) return;
    setTurns((t) => t.turnId === turnId, "rounds", produce((rounds) => rounds.push({ roundNum, thinking: undefined, answer: undefined, blocks: [], toolCalls: [], toolResults: [] })));
  }

  function handleTurnStart(turnId: string, userText: string) {
    resetStreamBuffer(); lastRoundNum = 0; setIsStreaming(true); setInputDisabled(true); setError(null);
    setTurns(produce((t) => t.push({ turnId, userText, rounds: [], status: "streaming" })));
  }

  let lastRoundNum = 0;

  function handleRoundDelta(turnId: string, roundNum: number, kind: string, delta: string) {
    // Reset stream buffer when entering a new round (e.g. after tool calls)
    if (roundNum !== lastRoundNum) {
      resetStreamBuffer();
      lastRoundNum = roundNum;
    }
    if (kind === "thinking") streamBuffer.thinking += delta; else if (kind === "answering") streamBuffer.answer += delta;
    ensureRound(turnId, roundNum);
    // RAF-batched: defer setTurns to next animation frame to coalesce rapid deltas
    pendingTurnId = turnId;
    pendingRoundNum = roundNum;
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

  function flushToolCallPreviews(turnId: string, roundNum: number) {
    const batch = [...pendingTcMap.values()];
    pendingTcMap.clear();
    if (batch.length === 0) return;
    setTurns((t) => t.turnId === turnId, "rounds", (r) => r.roundNum === roundNum, produce((round) => {
      for (const u of batch) {
        const existing = round.toolCalls.findIndex(tc => tc.id === u.id);
        if (existing >= 0) {
          round.toolCalls[existing].args_display = u.argsSoFar.slice(0, 100);
          round.toolCalls[existing].args_json = u.argsSoFar;
        } else {
          round.toolCalls.push({ id: u.id, name: u.name, args_display: u.argsSoFar.slice(0, 100), args_json: u.argsSoFar });
        }
      }
    }));
  }

  function handleToolCallPreview(turnId: string, roundNum: number, index: number, id: string, name: string, argsSoFar: string) {
    ensureRound(turnId, roundNum);
    pendingTcMap.set(id, { id, name, argsSoFar });
    if (!tcRafPending) {
      tcRafPending = true;
      requestAnimationFrame(() => {
        tcRafPending = false;
        flushToolCallPreviews(turnId, roundNum);
      });
    }
  }

  function handleRoundComplete(turnId: string, roundNum: number, thinking?: string, answer?: string, toolCalls?: ToolCallDef[], blocks?: RoundBlock[]) {
    flushDeltas(); // flush any pending streaming delta before replacing with final state
    ensureRound(turnId, roundNum);
    setTurns((t) => t.turnId === turnId, "rounds", (r) => r.roundNum === roundNum, produce((round) => {
      if (thinking) round.thinking = thinking; if (answer) round.answer = answer; if (toolCalls) round.toolCalls = toolCalls; if (blocks) round.blocks = blocks;
    }));
  }

  // ── RAF batching for exec progress (tool output streaming) ──
  let execRafPending = false;
  let pendingExecChunks = new Map<string, string>(); // toolCallId -> accumulated output

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
        const existing = round.toolResults.findIndex(tr => tr.tool_call_id === streamKey);
        if (existing >= 0) {
          round.toolResults[existing].output += output;
        } else {
          round.toolResults.push({ tool_call_id: streamKey, output, success: true });
        }
      }
    }));
  }

  function handleExecProgress(toolCallId: string, chunk: string) {
    // Guard: ignore if the last turn is already complete (prevents bleed into next round)
    const lastTurn = turns[turns.length - 1];
    if (!lastTurn || lastTurn.status !== "streaming") return;

    const prev = pendingExecChunks.get(toolCallId) || "";
    pendingExecChunks.set(toolCallId, prev + chunk);
    if (!execRafPending) {
      execRafPending = true;
      requestAnimationFrame(() => {
        execRafPending = false;
        flushExecProgress();
      });
    }
  }

  function handleToolResults(turnId: string, roundNum: number, results: ToolResultDef[]) {
    flushExecProgress(); // flush any pending exec progress before replacing with final results
    ensureRound(turnId, roundNum);
    setTurns((t) => t.turnId === turnId, "rounds", (r) => r.roundNum === roundNum, produce((round) => {
      // Remove streaming placeholders matching the final result IDs
      const resultIds = new Set(results.map(r => r.tool_call_id));
      round.toolResults = round.toolResults.filter(tr => !resultIds.has(tr.tool_call_id));
      // Push final results (ANSI rendering handled by ToolCallCard)
      round.toolResults.push(...results);
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

  function handleTurnEnd(turnId: string, data: Record<string, unknown>) {
    flushDeltas(); flushToolCallPreviews(turnId, lastRoundNum); flushExecProgress(); // flush all pending state before marking complete
    setIsStreaming(false); setInputDisabled(false); resetStreamBuffer(); lastRoundNum = 0;
    setTurns((t) => t.turnId === turnId, produce((turn) => { turn.status = "complete"; turn.stopReason = data.stop_reason as string | undefined; if (data.usage) turn.usage = data.usage as Turn["usage"]; }));
    const u = data.usage as Record<string, unknown> | undefined;
    if (u) {
      if (u.prompt_tokens != null) setSessionInfo("contextTokens", u.prompt_tokens as number);
      if (u.total_tokens != null) setSessionInfo("totalTokens", (s) => s + (u.total_tokens as number));
      if (u.prompt_cache_hit_tokens != null) setSessionInfo("promptCacheHit", u.prompt_cache_hit_tokens as number);
      if (u.prompt_cache_miss_tokens != null) setSessionInfo("promptCacheMiss", u.prompt_cache_miss_tokens as number);
    }
  }

  function handleSessionCreated(seed: string) {
    cacheCurrentStatus();           // save old session's tasks/activity/edits
    setSessionInfo("seed", seed);
    loadCachedStatus(seed);         // restore target session's data (or empty)
  }
  function handleDashboard(data: Record<string, unknown>) {
    if (data.session_seed) setSessionInfo("seed", data.session_seed as string);
    if (data.model) setSessionInfo("model", data.model as string);
    if (data.context_limit != null) setSessionInfo("contextLimit", data.context_limit as number);
    if (data.usage != null) {
      const u = data.usage as Record<string, unknown>;
      if (u.prompt_tokens != null) setSessionInfo("contextTokens", u.prompt_tokens as number);
      if (u.total_tokens != null) setSessionInfo("totalTokens", u.total_tokens as number);
      if (u.prompt_cache_hit_tokens != null) setSessionInfo("promptCacheHit", u.prompt_cache_hit_tokens as number);
      if (u.prompt_cache_miss_tokens != null) setSessionInfo("promptCacheMiss", u.prompt_cache_miss_tokens as number);
    }
    const newTasks = (data.tasks as TaskInfo[]) || [];
    const currentTasks = tasks();
    // Tag removed tasks for slide-out animation
    const newIds = new Set(newTasks.map(t => t.id));
    for (const t of currentTasks) {
      if (!newIds.has(t.id) && !(t as any)._deleting) {
        (t as any)._deleting = true;
      }
    }
    // Merge: keep deleting tasks for animation, add new/updated tasks
    const merged = newTasks.map(t => ({ ...t }));
    for (const t of currentTasks) {
      if ((t as any)._deleting && !newIds.has(t.id)) {
        merged.push({ ...t, _deleting: true } as any);
      }
    }
    setTasks(merged as TaskInfo[]);
    // Remove after animation
    for (const t of currentTasks) {
      if ((t as any)._deleting && !newIds.has(t.id)) {
        setTimeout(() => {
          setTasks((prev: TaskInfo[]) => prev.filter((x: TaskInfo) => x.id !== t.id));
        }, 400);
      }
    }
    if (data.recent_edits != null) setRecentEdits(data.recent_edits as string[]);
  }

  function handleCancelled() { setIsStreaming(false); setInputDisabled(false); resetStreamBuffer(); }
  function handleDone() { setIsStreaming(false); }
  function handleError(message: string) {
    setError(message); setIsStreaming(false); setInputDisabled(false);
    const lastTurn = turns[turns.length - 1];
    if (lastTurn && lastTurn.status === "streaming") setRestoreText(lastTurn.userText);
  }
  function clearError() { setError(null); }
  function handleAuditRecord(data: { tool_name: string; result_summary: string; success: boolean }) {
    setActivityLog((prev) => {
      const next = [{ tool_name: data.tool_name, summary: data.result_summary, success: data.success, time: Date.now() }, ...prev];
      return next.length > 50 ? next.slice(0, 50) : next;
    });
  }
  function clear() { setTurns([]); setError(null); setTasks([]); setRecentEdits([]); setActivityLog([]); resetStreamBuffer(); setIsStreaming(false); setInputDisabled(false); lastRoundNum = 0; }
  function clearTurns() { setTurns([]); setError(null); resetStreamBuffer(); }

  async function undoTurn(turnId: string) {
    try {
      await invoke("cmd_undo_turn", { seed, turnId });
    } catch (e) { console.error(e); }
    const num = parseInt(turnId.replace("t", ""), 10);
    if (!isNaN(num)) {
      setTurns((prev) => prev.filter((t) => parseInt(t.turnId.replace("t", ""), 10) < num));
    }
  }

  function handleCompactStart(_data: Record<string, unknown>) {
    setIsCompacting(true);
    setCompactResult(null);
  }
  function handleCompactEnd(data: Record<string, unknown>) {
    setIsCompacting(false);
    const chars = data.summary_chars as number;
    const turns = data.turns_compacted as number;
    if (chars > 0) {
      setCompactResult(`Compacted ${turns} turns → ${chars} char summary`);
      setTimeout(() => setCompactResult(null), 4000);
    }
  }

  function handleToolNotice(data: Record<string, unknown>) {
    const msg = (data.message ?? "") as string;
    setCompactResult(msg);
    setTimeout(() => setCompactResult(null), 4000);
  }

  // Load session data from disk (for resume / refresh restore)
  function loadSessionFromData(sessionJson: string) {
    try {
      const session = JSON.parse(sessionJson);
      if (!session.messages) return;
      const messages: Array<{ role: string; content: Array<{ type: string; text?: string; reasoning?: string; id?: string; name?: string; input?: unknown; tool_use_id?: string; content?: string }> }> = session.messages;
      const loaded: Turn[] = [];
      let currentTurn: Turn | null = null;
      let roundNum = 0;
      let turnIdx = 0;

      for (const msg of messages) {
        if (msg.role === "system") continue;
        if (msg.role === "user") {
          currentTurn = {
            turnId: `t${++turnIdx}`,
            userText: msg.content.find((b) => b.type === "text")?.text ?? "",
            rounds: [],
            status: "complete",
          };
          loaded.push(currentTurn);
          roundNum = 0;
        } else if (msg.role === "assistant" && currentTurn) {
          roundNum++;
          const thinking = msg.content.find((b) => b.type === "reasoning")?.reasoning;
          const answer = msg.content.find((b) => b.type === "text")?.text;
          const toolCalls: ToolCallDef[] = msg.content
            .filter((b) => b.type === "tool_use")
            .map((b) => ({ id: b.id ?? "", name: b.name ?? "", args_display: b.name ?? "", args_json: JSON.stringify(b.input ?? {}) }));
 const blocks: RoundBlock[] = msg.content.map((b) => {
            if (b.type === "text") return { type: "text", content: b.text ?? "" };
            if (b.type === "tool_use") return { type: "tool", card: { id: b.id ?? "", name: b.name ?? "", args_display: b.name ?? "", args_json: JSON.stringify(b.input ?? {}) } };
            return { type: "text", content: "" };
          });
          currentTurn.rounds.push({ roundNum, thinking, answer, blocks, toolCalls, toolResults: [] });
        } else if (msg.role === "tool" && currentTurn) {
          const lastRound = currentTurn.rounds[currentTurn.rounds.length - 1];
          if (lastRound) {
            for (const block of msg.content) {
              if (block.type === "tool_result") {
                lastRound.toolResults.push({ tool_call_id: block.tool_use_id ?? "", output: block.content ?? "", success: true });
              }
            }
          }
        }
      }
      setTurns(loaded);
      return loaded.length;
    } catch (e) {
      console.error("Failed to load session data:", e);
      return 0;
    }
  }

  // Load turns from SessionRestored agent event (authoritative restored state).
  function loadTurnsFromRestore(turnsData: Array<{
    turn_id: string; user_text: string; rounds: Array<{
      round_num: number; thinking?: string; answer?: string;
      tool_calls: ToolCallDef[]; tool_results: ToolResultDef[];
    }>;
  }>) {
    const loaded: Turn[] = turnsData.map((td) => {
      const rounds: Round[] = td.rounds.map((rd) => {
        const blocks: RoundBlock[] = [];
        if (rd.thinking) blocks.push({ type: "reasoning", content: rd.thinking });
        if (rd.answer) blocks.push({ type: "text", content: rd.answer });
        for (const tc of rd.tool_calls) blocks.push({ type: "tool", card: tc });
        return { roundNum: rd.round_num, thinking: rd.thinking, answer: rd.answer, blocks, toolCalls: rd.tool_calls, toolResults: rd.tool_results };
      });
      return { turnId: td.turn_id, userText: td.user_text, rounds, status: "complete" };
    });
    setTurns(loaded);
  }

  function prependTurns(turnsData: Array<{
    turn_id: string; user_text: string; rounds: Array<{
      round_num: number; thinking?: string; answer?: string;
      tool_calls: ToolCallDef[]; tool_results: ToolResultDef[];
    }>;
  }>) {
    const loaded: Turn[] = turnsData.map((td) => {
      const rounds: Round[] = td.rounds.map((rd) => {
        const blocks: RoundBlock[] = [];
        if (rd.thinking) blocks.push({ type: "reasoning", content: rd.thinking });
        if (rd.answer) blocks.push({ type: "text", content: rd.answer });
        for (const tc of rd.tool_calls) blocks.push({ type: "tool", card: tc });
        return { roundNum: rd.round_num, thinking: rd.thinking, answer: rd.answer, blocks, toolCalls: rd.tool_calls, toolResults: rd.tool_results };
      });
      return { turnId: td.turn_id, userText: td.user_text, rounds, status: "complete" as const };
    });
    setTurns(produce((prev) => {
      prev.unshift(...loaded);
    }));
  }

  async function submitAskAnswer(answer: string) {
    setAskState({ question: "", options: [], allow_custom: true, show: false });
    try {
      await invoke("cmd_send_message", { seed, text: answer });
    } catch (e) { console.error(e); }
  }

  function dismissAsk() { setAskState({ question: "", options: [], allow_custom: true, show: false }); }

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

 return { turns, sessionInfo, isStreaming, inputDisabled, hasMore, setHasMore, workspace, setWorkspace, error, restoreText, tasks, recentEdits, activityLog, askState, submitAskAnswer, dismissAsk, submitTaskAction, isCompacting, compactResult, codeDeltas, setCodeDeltas, handleCompactStart, handleCompactEnd, handleToolNotice, handleTurnStart, handleRoundDelta, handleToolCallPreview, handleRoundComplete, handleToolResults, handleExecProgress, handleTurnEnd, handleSessionCreated, handleDashboard, handleAuditRecord, handleCancelled, handleDone, handleError, clearError, clear, clearTurns, undoTurn, setInputDisabled, loadSessionFromData, loadTurnsFromRestore, prependTurns };
}