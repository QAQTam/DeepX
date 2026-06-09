import { createStore, produce } from "solid-js/store";
import { createSignal } from "solid-js";

export interface ToolCallDef { id: string; name: string; args_display: string; args_json: string; }
export interface ToolResultDef { tool_call_id: string; output: string; success: boolean; }
export interface RoundBlock { type: "reasoning" | "text" | "tool"; content?: string; card?: ToolCallDef; }
export interface Round { roundNum: number; thinking?: string; answer?: string; blocks: RoundBlock[]; toolCalls: ToolCallDef[]; toolResults: ToolResultDef[]; }
export interface Turn { turnId: string; userText: string; rounds: Round[]; status: "streaming" | "complete"; stopReason?: string; usage?: { input_tokens: number; output_tokens: number; total_tokens: number }; }
export interface SessionInfo { seed: string; model: string; contextTokens: number; contextLimit: number; totalTokens: number; promptCacheHit: number; promptCacheMiss: number; }
export interface SessionMeta { seed: string; model: string; created_at: number; updated_at: number; message_count: number; last_summary: string; }
export interface TaskInfo { id: string; subject: string; description: string; status: string; }
export interface ActivityEntry { tool_name: string; summary: string; success: boolean; time: number; }

export function createChatStore() {
  const [turns, setTurns] = createStore<Turn[]>([]);
  const [sessionInfo, setSessionInfo] = createStore<SessionInfo>({ seed: "", model: "", contextTokens: 0, contextLimit: 0, totalTokens: 0, promptCacheHit: 0, promptCacheMiss: 0 });
  const [isStreaming, setIsStreaming] = createSignal(false);
  const [inputDisabled, setInputDisabled] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [tasks, setTasks] = createSignal<TaskInfo[]>([]);
  const [recentEdits, setRecentEdits] = createSignal<string[]>([]);
  const [activityLog, setActivityLog] = createSignal<ActivityEntry[]>([]);
  let streamBuffer = { thinking: "", answer: "" };

  function resetStreamBuffer() { streamBuffer = { thinking: "", answer: "" }; }

  function ensureRound(turnId: string, roundNum: number) {
    const turn = turns.find((t) => t.turnId === turnId);
    if (!turn) return;
    if (turn.rounds.find((r) => r.roundNum === roundNum)) return;
    setTurns((t) => t.turnId === turnId, "rounds", produce((rounds) => rounds.push({ roundNum, thinking: undefined, answer: undefined, blocks: [], toolCalls: [], toolResults: [] })));
  }

  function handleTurnStart(turnId: string, userText: string) {
    resetStreamBuffer(); setIsStreaming(true); setInputDisabled(true); setError(null);
    setTurns(produce((t) => t.push({ turnId, userText, rounds: [], status: "streaming" })));
  }

  function handleRoundDelta(turnId: string, roundNum: number, kind: string, delta: string) {
    if (kind === "thinking") streamBuffer.thinking += delta; else if (kind === "answering") streamBuffer.answer += delta;
    ensureRound(turnId, roundNum);
    setTurns((t) => t.turnId === turnId, "rounds", (r) => r.roundNum === roundNum, produce((round) => {
      if (kind === "thinking") round.thinking = streamBuffer.thinking; if (kind === "answering") round.answer = streamBuffer.answer;
    }));
  }

  function handleRoundComplete(turnId: string, roundNum: number, thinking?: string, answer?: string, toolCalls?: ToolCallDef[], blocks?: RoundBlock[]) {
    ensureRound(turnId, roundNum);
    setTurns((t) => t.turnId === turnId, "rounds", (r) => r.roundNum === roundNum, produce((round) => {
      if (thinking) round.thinking = thinking; if (answer) round.answer = answer; if (toolCalls) round.toolCalls = toolCalls; if (blocks) round.blocks = blocks;
    }));
  }

  function handleToolResults(turnId: string, roundNum: number, results: ToolResultDef[]) {
    ensureRound(turnId, roundNum);
    setTurns((t) => t.turnId === turnId, "rounds", (r) => r.roundNum === roundNum, "toolResults", produce((tr) => tr.push(...results)));
  }

  function handleTurnEnd(turnId: string, data: Record<string, unknown>) {
    setIsStreaming(false); setInputDisabled(false); resetStreamBuffer();
    setTurns((t) => t.turnId === turnId, produce((turn) => { turn.status = "complete"; turn.stopReason = data.stop_reason as string | undefined; if (data.usage) turn.usage = data.usage as Turn["usage"]; }));
    if (data.usage != null) { const u = data.usage as Record<string, unknown>; if (u.total_tokens != null) setSessionInfo("totalTokens", u.total_tokens as number); }
    if (data.context_tokens != null) { setSessionInfo("contextTokens", data.context_tokens as number); setSessionInfo("contextLimit", (data.context_limit as number) ?? 0); }
  }

  function handleSessionCreated(seed: string) { setSessionInfo("seed", seed); }
  function handleDashboard(data: Record<string, unknown>) {
    if (data.session_seed) setSessionInfo("seed", data.session_seed as string);
    if (data.context_tokens != null) setSessionInfo("contextTokens", data.context_tokens as number);
    if (data.prompt_cache_hit_tokens != null) setSessionInfo("promptCacheHit", data.prompt_cache_hit_tokens as number);
    if (data.prompt_cache_miss_tokens != null) setSessionInfo("promptCacheMiss", data.prompt_cache_miss_tokens as number);
    if (data.tasks != null) setTasks(data.tasks as TaskInfo[]);
    if (data.recent_edits != null) setRecentEdits(data.recent_edits as string[]);
  }

  function handleCancelled() { setIsStreaming(false); setInputDisabled(false); resetStreamBuffer(); }
  function handleError(message: string) { setError(message); setIsStreaming(false); setInputDisabled(false); }
  function handleAuditRecord(data: { tool_name: string; result_summary: string; success: boolean }) {
    setActivityLog((prev) => {
      const next = [{ tool_name: data.tool_name, summary: data.result_summary, success: data.success, time: Date.now() }, ...prev];
      return next.length > 50 ? next.slice(0, 50) : next;
    });
  }
  function clear() { setTurns([]); setError(null); setTasks([]); setRecentEdits([]); setActivityLog([]); resetStreamBuffer(); }

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
            if (b.type === "reasoning") return { type: "reasoning", content: b.reasoning ?? "" };
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

  return { turns, sessionInfo, isStreaming, inputDisabled, error, tasks, recentEdits, activityLog, handleTurnStart, handleRoundDelta, handleRoundComplete, handleToolResults, handleTurnEnd, handleSessionCreated, handleDashboard, handleAuditRecord, handleCancelled, handleError, clear, setInputDisabled, loadSessionFromData };
}
