import type { Agent2Ui, RoundData, TurnData } from "../lib/types";
import {
  emptyRawRound,
  type RawRound,
  type RawSessionState,
  type RawTurn,
} from "./rawSession";

export function createRawSessionState(seed: string): RawSessionState {
  return {
    seed,
    turns: [],
    pendingInteraction: null,
    environment: {
      linesAdded: 0,
      linesRemoved: 0,
      filesCreated: 0,
      filesDeleted: 0,
      changedFiles: [],
    },
    session: {
      ready: false,
      hasMore: false,
      totalTurns: 0,
      tokensUsed: 0,
      cacheHitPct: 0,
      contextLimit: 0,
    },
    skills: { available: [], active: [] },
    notices: [],
    compact: { active: false, text: "" },
  };
}

export function assertNever(value: never): never {
  throw new Error(`Unhandled Agent2Ui event: ${JSON.stringify(value)}`);
}

function restoredRound(round: RoundData): RawRound {
  return {
    ...emptyRawRound(round.round_num),
    isFinal: round.is_final,
    thinking: round.thinking ?? "",
    answer: round.answer ?? "",
    toolCalls: round.tool_calls,
    toolResults: Object.fromEntries(round.tool_results.map(result => [result.tool_call_id, result])),
  };
}

function restoredTurn(turn: TurnData): RawTurn {
  return {
    turnId: turn.turn_id,
    userText: turn.user_text,
    status: "completed",
    rounds: turn.rounds.map(restoredRound),
    interactions: [],
  };
}

function updateTurn(
  state: RawSessionState,
  turnId: string,
  update: (turn: RawTurn) => RawTurn,
): RawSessionState {
  return {
    ...state,
    turns: state.turns.map(turn => turn.turnId === turnId ? update(turn) : turn),
  };
}

function updateRound(
  state: RawSessionState,
  turnId: string,
  roundNum: number,
  update: (round: RawRound) => RawRound,
): RawSessionState {
  return updateTurn(state, turnId, turn => {
    const exists = turn.rounds.some(round => round.roundNum === roundNum);
    const rounds = exists ? turn.rounds : [...turn.rounds, emptyRawRound(roundNum)];
    return {
      ...turn,
      rounds: rounds.map(round => round.roundNum === roundNum ? update(round) : round),
    };
  });
}

function lastTurnId(state: RawSessionState): string | undefined {
  return state.turns[state.turns.length - 1]?.turnId;
}

function closePendingInteraction(
  state: RawSessionState,
  id: string,
  resolution: string,
  now: number,
): RawSessionState {
  const pending = state.pendingInteraction;
  if (!pending || pending.id !== id) return state;
  const turnId = pending.kind === "ask" ? pending.turnId : lastTurnId(state);
  const next = { ...state, pendingInteraction: null };
  if (!turnId) return next;
  return updateTurn(next, turnId, turn => ({
    ...turn,
    interactions: [...turn.interactions, { id, kind: pending.kind, resolution, at: now }],
  }));
}

export function resolvePendingInteraction(
  state: RawSessionState,
  id: string,
  resolution: string,
  now = Date.now(),
): RawSessionState {
  const next = closePendingInteraction(state, id, resolution, now);
  const turnId = lastTurnId(next);
  if (!turnId) return next;
  return updateTurn(next, turnId, turn => turn.status === "waiting" ? { ...turn, status: "running" } : turn);
}

export function reduceAgentEvent(
  state: RawSessionState,
  event: Agent2Ui,
  now = Date.now(),
): RawSessionState {
  switch (event.type) {
    case "turn_start":
      if (state.turns.some(turn => turn.turnId === event.turn_id)) return state;
      return {
        ...state,
        turns: [...state.turns, {
          turnId: event.turn_id,
          userText: event.user_text,
          status: "running",
          startedAt: now,
          rounds: [],
          interactions: [],
        }],
      };
    case "turn_end":
      return updateTurn(state, event.turn_id, turn => ({
        ...turn,
        status: turn.status === "failed" || turn.status === "cancelled" ? turn.status : "completed",
        endedAt: now,
        stopReason: event.stop_reason,
        usage: event.usage,
      }));
    case "round_delta":
      return updateRound(state, event.turn_id, event.round_num, round => ({
        ...round,
        thinking: event.kind === "thinking" ? round.thinking + event.delta : round.thinking,
        answer: event.kind === "answering" ? round.answer + event.delta : round.answer,
      }));
    case "round_complete":
      return updateRound(state, event.turn_id, event.round_num, round => ({
        ...round,
        isFinal: event.is_final,
        thinking: event.thinking ?? round.thinking,
        answer: event.answer ?? round.answer,
        toolCalls: event.tool_calls ?? [],
        blocks: event.blocks ?? [],
      }));
    case "tool_results":
      return updateRound(state, event.turn_id, event.round_num, round => ({
        ...round,
        toolResults: {
          ...round.toolResults,
          ...Object.fromEntries(event.results.map(result => [result.tool_call_id, result])),
        },
      }));
    case "tool_exec_delta": {
      const turnId = lastTurnId(state);
      if (!turnId) return state;
      const turn = state.turns.find(item => item.turnId === turnId);
      const roundNum = turn?.rounds[turn.rounds.length - 1]?.roundNum ?? 0;
      return updateRound(state, turnId, roundNum, round => {
        const previous = round.progress[event.tool_call_id]?.chunks ?? [];
        return {
          ...round,
          progress: {
            ...round.progress,
            [event.tool_call_id]: {
              chunks: [...previous, {
                stream: "stdout" as const,
                seq: previous.length,
                chunk: event.delta,
              }],
            },
          },
        };
      });
    }
    case "exec_progress": {
      const turn = [...state.turns].reverse().find(candidate =>
        candidate.rounds.some(round => round.toolCalls.some(call => call.id === event.tool_call_id)),
      ) ?? state.turns[state.turns.length - 1];
      if (!turn) return state;
      const round = [...turn.rounds].reverse().find(candidate =>
        candidate.toolCalls.some(call => call.id === event.tool_call_id),
      ) ?? turn.rounds[turn.rounds.length - 1] ?? emptyRawRound(0);
      return updateRound(state, turn.turnId, round.roundNum, current => {
        const previous = current.progress[event.tool_call_id]?.chunks ?? [];
        const chunks = [
          ...previous.filter(item => item.seq !== Number(event.seq)),
          {
            stream: event.stream === "stderr" ? "stderr" as const : "stdout" as const,
            seq: Number(event.seq),
            chunk: event.chunk,
          },
        ].sort((a, b) => a.seq - b.seq);
        return {
          ...current,
          progress: { ...current.progress, [event.tool_call_id]: { chunks } },
        };
      });
    }
    case "tool_call_preview":
      return updateRound(state, event.turn_id, event.round_num, round => {
        const preview = {
          id: event.id,
          name: event.name,
          args_display: event.args_so_far.slice(0, 100),
          args_json: event.args_so_far,
        };
        const exists = round.toolCalls.some(call => call.id === event.id);
        return {
          ...round,
          toolCalls: exists
            ? round.toolCalls.map(call => call.id === event.id ? preview : call)
            : [...round.toolCalls, preview],
        };
      });
    case "session_restored":
      return {
        ...state,
        seed: event.seed,
        turns: event.turns.map(restoredTurn),
        session: {
          ...state.session,
          totalTurns: event.total_turns,
          hasMore: event.has_more,
          tokensUsed: event.tokens_used,
          cacheHitPct: event.cache_hit_pct,
        },
      };
    case "more_turns":
      return {
        ...state,
        turns: [...event.turns.map(restoredTurn), ...state.turns],
        session: { ...state.session, hasMore: event.has_more },
      };
    case "session_created":
      return { ...createRawSessionState(event.seed), session: { ...state.session, ready: true } };
    case "error": {
      const turnId = lastTurnId(state);
      const next = {
        ...state,
        notices: [...state.notices, { level: "error", message: event.message, at: now }],
      };
      return turnId ? updateTurn(next, turnId, turn => ({ ...turn, status: "failed", endedAt: now })) : next;
    }
    case "tool_notice":
      return { ...state, notices: [...state.notices, { level: event.level, message: event.message, at: now }] };
    case "dashboard":
      return {
        ...state,
        session: {
          ...state.session,
          title: event.session_title,
          model: event.model,
          contextLimit: event.context_limit,
        },
      };
    case "code_delta":
      return {
        ...state,
        environment: {
          linesAdded: state.environment.linesAdded + event.lines_added,
          linesRemoved: state.environment.linesRemoved + event.lines_removed,
          filesCreated: state.environment.filesCreated + event.files_created,
          filesDeleted: state.environment.filesDeleted + event.files_deleted,
          changedFiles: event.file && !state.environment.changedFiles.includes(event.file)
            ? [...state.environment.changedFiles, event.file]
            : state.environment.changedFiles,
        },
      };
    case "skills_changed":
      return { ...state, skills: { available: event.available, active: event.active } };
    case "permission_request": {
      const turnId = lastTurnId(state);
      const next = {
        ...state,
        pendingInteraction: {
          kind: "permission" as const,
          id: event.tool_call_id,
          toolName: event.tool_name,
          reason: event.reason,
          paths: event.paths,
          category: event.category,
          level: event.level,
          risk: event.risk,
          consequence: event.consequence,
        },
      };
      return turnId ? updateTurn(next, turnId, turn => ({ ...turn, status: "waiting" })) : next;
    }
    case "ask_user":
      return updateTurn({
        ...state,
        pendingInteraction: {
          kind: "ask",
          id: event.ask_id,
          turnId: event.turn_id,
          roundNum: event.round_num,
          mode: event.mode,
          questions: event.questions,
        },
      }, event.turn_id, turn => ({ ...turn, status: "waiting" }));
    case "ask_resolved":
      return resolvePendingInteraction(state, event.ask_id, event.resolution, now);
    case "ask_rejected":
      return {
        ...state,
        notices: [...state.notices, { level: "error", message: event.message, at: now }],
      };
    case "compact_start":
      return { ...state, compact: { active: true, text: "" } };
    case "compact_delta":
      return { ...state, compact: { active: true, text: state.compact.text + event.delta } };
    case "compact_end":
      return { ...state, compact: { active: false, text: "" } };
    case "cancelled": {
      const turnId = lastTurnId(state);
      const next = { ...state, pendingInteraction: null };
      return turnId ? updateTurn(next, turnId, turn => ({ ...turn, status: "cancelled", endedAt: now })) : next;
    }
    case "ready":
      return { ...state, session: { ...state.session, ready: true } };
    case "done":
    case "shutdown_ack":
    case "pong":
    case "plan_changed":
    case "audit_record":
      return state;
    default:
      return assertNever(event);
  }
}
