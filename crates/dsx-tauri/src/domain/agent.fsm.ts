// ── Agent Finite State Machine ──
// Pure functions, zero React dependency. Fully testable.

import type { StreamKind } from '../types/agent'

// ── States ──

export type AgentStatus = 'idle' | 'connecting' | 'ready' | 'streaming' | 'error'

export interface StreamState {
  content: string
  reasoning: string
  toolCards: ToolCardEntry[]
  kind: StreamKind | null
  toolNames: string[]
  final: boolean
}

export interface ToolCardEntry {
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
  stream: StreamState
  sessions: SessionMeta[]
  connected: boolean
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
  | { type: 'STREAM_START'; kind: StreamKind; toolNames?: string[] }
  | { type: 'STREAM_DELTA'; delta: string; kind?: 'thinking' | 'content' }
  | { type: 'STREAM_END'; isFinal: boolean }
  | { type: 'FLUSH_STREAM' }
  | { type: 'CANCEL' }
  | { type: 'RESTORE_SESSION'; seed: string }

// ── Initial state ──

export function createInitialState(): AgentState {
  return {
    status: 'idle',
    sessionId: null,
    error: null,
    stream: {
      content: '',
      reasoning: '',
      toolCards: [],
      kind: null,
      toolNames: [],
      final: false,
    },
    sessions: [],
    connected: false,
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
        stream: createInitialState().stream,
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
        stream: createInitialState().stream,
        sessionId: null,
      }

    case 'SHUTDOWN_ACK':
      return {
        ...state,
        status: 'idle',
        connected: false,
        stream: createInitialState().stream,
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

      case 'STREAM_START':
        return {
          ...state,
          status: 'streaming' as const,
          stream: {
            ...state.stream,
            content: '',
            // Preserve reasoning across phase switches
            kind: action.kind,
            toolNames: action.toolNames || [],
            final: false,
          },
        }

    case 'STREAM_DELTA': {
      const stream = { ...state.stream }
      const kind = action.kind ?? (state.stream.kind === 'thinking' ? 'thinking' : 'content')
      if (kind === 'thinking') {
        stream.reasoning += action.delta
      } else {
        stream.content += action.delta
      }
      return { ...state, stream }
    }

    case 'STREAM_END':
      return {
        ...state,
        stream: { ...state.stream, final: action.isFinal },
        status: !action.isFinal ? state.status : 'ready',
      }

    case 'FLUSH_STREAM':
      return {
        ...state,
        status: 'ready' as const,
        stream: createInitialState().stream,
      }

    case 'CANCEL':
      return {
        ...state,
        status: 'ready',
        stream: createInitialState().stream,
      }

    case 'RESTORE_SESSION':
      return {
        ...state,
        sessionId: action.seed,
      }

    default:
      return state
  }
}