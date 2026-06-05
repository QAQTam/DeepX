// ── Agent FSM v5 (round-based) ──
// Pure functions for managing agent state.

import type { TurnData, RoundData, ToolCallDef, ToolResultDef } from '../types/agent'

// ── States ──

export type AgentStatus = 'idle' | 'connecting' | 'ready' | 'streaming' | 'error'

export interface TourCardEntry {
  id?: string
  name: string
  args: string
  body?: unknown
  output?: string
  success?: boolean
}

export interface AgentState {
  status: AgentStatus
  sessionId: string | null
  error: string | null
  turns: TurnData[]
  sessions: SessionMeta[]
  connected: boolean
  streaming: boolean
}

export interface SessionMeta {
  seed: string
  date?: string
  model?: string
  message_count?: number
}

// ── Actions ──

export type AgentAction =
  | { type: 'START_CONNECT' }
  | { type: 'CONNECTED'; seed?: string; sessions: SessionMeta[] }
  | { type: 'DISCONNECT' }
  | { type: 'SHUTDOWN_ACK' }
  | { type: 'AGENT_CLOSED' }
  | { type: 'ERROR'; message: string }
  | { type: 'TURN_START'; turn_id: string; user_text: string }
  | { type: 'ROUND_COMPLETE'; turn_id: string; round_num: number; thinking?: string; answer?: string; tool_calls?: ToolCallDef[]; is_final: boolean }
  | { type: 'TOOL_RESULTS'; turn_id: string; round_num: number; results: ToolResultDef[] }
  | { type: 'TURN_END'; turn_id: string; usage?: unknown; context_tokens?: number; context_limit?: number; session_tokens?: number }
  | { type: 'CANCEL' }
  | { type: 'RESTORE_SESSION'; seed: string; turns: TurnData[] }

// ── Initial state ──

export function createInitialState(): AgentState {
  return {
    status: 'idle',
    sessionId: null,
    error: null,
    turns: [],
    sessions: [],
    connected: false,
    streaming: false,
  }
}

// ── Reducer (pure) ──

export function agentReducer(state: AgentState, action: AgentAction): AgentState {
  switch (action.type) {
    case 'START_CONNECT':
      return {
        ...state,
        status: 'connecting',
        error: null,
        turns: [],
      }

    case 'CONNECTED':
      return {
        ...state,
        status: 'ready',
        sessionId: action.seed || null,
        sessions: action.sessions,
        connected: true,
        error: null,
      }

    case 'DISCONNECT':
      return {
        ...state,
        status: 'idle',
        connected: false,
        sessionId: null,
      }

    case 'SHUTDOWN_ACK':
      return {
        ...state,
        status: 'idle',
        connected: false,
      }

    case 'AGENT_CLOSED':
      return {
        ...state,
        status: state.status === 'connecting' ? 'idle' : state.status,
        connected: state.status !== 'connecting',
      }

    case 'ERROR':
      return {
        ...state,
        status: 'error',
        error: action.message,
        connected: false,
      }

    case 'TURN_START': {
      const newTurn: TurnData = {
        turn_id: action.turn_id,
        user_text: action.user_text,
        rounds: [],
      }
      return {
        ...state,
        status: 'streaming',
        streaming: true,
        turns: [...state.turns, newTurn],
      }
    }

    case 'ROUND_COMPLETE': {
      const turns = [...state.turns]
      const turnIdx = turns.findIndex(t => t.turn_id === action.turn_id)
      if (turnIdx < 0) return state
      const turn = { ...turns[turnIdx] }
      const newRound: RoundData = {
        round_num: action.round_num,
        thinking: action.thinking || null,
        answer: action.answer || null,
        tool_calls: action.tool_calls || [],
        tool_results: [],
      }
      turn.rounds = [...turn.rounds, newRound]
      turns[turnIdx] = turn
      return {
        ...state,
        turns,
        streaming: !action.is_final,
        status: action.is_final ? 'ready' : 'streaming',
      }
    }

    case 'TOOL_RESULTS': {
      const turns = [...state.turns]
      const turnIdx = turns.findIndex(t => t.turn_id === action.turn_id)
      if (turnIdx < 0) return state
      const turn = { ...turns[turnIdx] }
      const roundIdx = turn.rounds.length - 1 // last round
      if (roundIdx < 0) return state
      const round = { ...turn.rounds[roundIdx] }
      round.tool_results = action.results
      turn.rounds = [...turn.rounds.slice(0, roundIdx), round, ...turn.rounds.slice(roundIdx + 1)]
      turns[turnIdx] = turn
      return { ...state, turns }
    }

    case 'TURN_END':
      return {
        ...state,
        streaming: false,
        status: 'ready',
      }

    case 'CANCEL':
      return {
        ...state,
        status: 'ready',
        streaming: false,
      }

    case 'RESTORE_SESSION':
      return {
        ...state,
        sessionId: action.seed,
        turns: action.turns,
      }

    default:
      return state
  }
}
