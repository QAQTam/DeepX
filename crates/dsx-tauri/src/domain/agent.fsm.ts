// ── Agent FSM v5 (connection lifecycle only) ──

export type AgentStatus = 'idle' | 'connecting' | 'ready' | 'streaming' | 'error'

export interface SessionMeta {
  seed: string
  date?: string
  model?: string
  message_count?: number
}

export interface AgentState {
  status: AgentStatus
  sessionId: string | null
  error: string | null
  sessions: SessionMeta[]
  connected: boolean
  streaming: boolean
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
  | { type: 'TURN_END'; turn_id: string }
  | { type: 'CANCEL' }
  | { type: 'RESTORE_SESSION'; seed: string; turns?: unknown[] }

// ── Initial state ──

export function createInitialState(): AgentState {
  return {
    status: 'idle',
    sessionId: null,
    error: null,
    sessions: [],
    connected: false,
    streaming: false,
  }
}

// ── Reducer (pure) ──

export function agentReducer(state: AgentState, action: AgentAction): AgentState {
  switch (action.type) {
    case 'START_CONNECT':
      return { ...state, status: 'connecting', error: null }

    case 'CONNECTED':
      return { ...state, status: 'ready', sessionId: action.seed || null, sessions: action.sessions, connected: true, error: null }

    case 'DISCONNECT':
      return { ...state, status: 'idle', connected: false, sessionId: null }

    case 'SHUTDOWN_ACK':
      return { ...state, status: 'idle', connected: false }

    case 'AGENT_CLOSED':
      return { ...state, status: state.status === 'connecting' ? 'idle' : state.status, connected: state.status !== 'connecting' }

    case 'ERROR':
      return { ...state, status: 'error', error: action.message, connected: false }

    case 'TURN_START':
      return { ...state, status: 'streaming', streaming: true }

    case 'TURN_END':
      return { ...state, streaming: false, status: 'ready' }

    case 'CANCEL':
      return { ...state, status: 'ready', streaming: false }

    case 'RESTORE_SESSION':
      return { ...state, sessionId: action.seed }

    default:
      return state
  }
}
