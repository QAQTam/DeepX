// ── Agent Event Types v5 (round-based protocol) ──

// ── Core data structures ──

export interface ToolCallDef {
  id: string
  name: string
  args_display: string
  args_json: string
}

export interface ToolResultDef {
  tool_call_id: string
  output: string
  success: boolean
}

export interface RoundData {
  round_num: number
  thinking: string | null
  answer: string | null
  tool_calls: ToolCallDef[]
  tool_results: ToolResultDef[]
}

export interface TurnData {
  turn_id: string
  user_text: string
  rounds: RoundData[]
}

// ── Event payloads ──

export type RoundDeltaKind = 'thinking' | 'tool_calling' | 'answering'

export interface TurnStartPayload {
  type: 'turn_start'
  turn_id: string
  user_text: string
}

export interface TurnEndPayload {
  type: 'turn_end'
  turn_id: string
  stop_reason?: string
  usage?: {
    prompt_tokens: number
    completion_tokens: number
    total_tokens: number
    prompt_cache_hit_tokens: number
    prompt_cache_miss_tokens: number
    reasoning_tokens: number
  }
  context_tokens: number
  context_limit: number
  session_tokens: number
}

export interface RoundDeltaPayload {
  type: 'round_delta'
  turn_id: string
  round_num: number
  kind: RoundDeltaKind
  delta: string
}

export interface RoundCompletePayload {
  type: 'round_complete'
  turn_id: string
  round_num: number
  thinking?: string
  answer?: string
  tool_calls?: ToolCallDef[]
  is_final: boolean
}

export interface ToolResultsPayload {
  type: 'tool_results'
  turn_id: string
  round_num: number
  results: ToolResultDef[]
}

export interface SessionRestoredPayload {
  type: 'session_restored'
  seed: string
  turns: TurnData[]
  tokens_used: number
  cache_hit_pct: number
}

export interface AskUserPayload {
  type: 'ask_user'
  id: string
  question: string
  options?: string[]
}

export interface ErrorPayload {
  type: 'error'
  message: string
}

export interface BalancePayload {
  type: 'balance'
  is_available: boolean
  total_balance: string
  currency: string
}

export interface DebugSnapshotPayload {
  type: 'debug_snapshot'
  hp_connected: boolean
  session_seed: string
  context_tokens: number
  tool_calls_total: number
  tool_failures: number
  current_phase: string
  streaming: boolean
  dsml_compat_count: number
  documents?: { tag: string; path: string; turns_since_read: number; is_stale: boolean }[]
  recent_edits?: string[]
  tasks?: { subject: string; description: string; status: string }[]
  session_title?: string
  prompt_cache_hit_tokens: number
  prompt_cache_miss_tokens: number
}

export interface AuditRecordPayload {
  type: 'audit_record'
  tool_name: string
  result_summary: string
  success: boolean
}

export interface ToolNoticePayload {
  type: 'tool_notice'
  message: string
  level: string
}

// ── Union type ──

export type AgentEvent =
  | TurnStartPayload
  | TurnEndPayload
  | RoundDeltaPayload
  | RoundCompletePayload
  | ToolResultsPayload
  | SessionRestoredPayload
  | AskUserPayload
  | ErrorPayload
  | BalancePayload
  | DebugSnapshotPayload
  | AuditRecordPayload
  | ToolNoticePayload
  | { type: 'done' }
  | { type: 'cancelled' }
  | { type: 'shutdown_ack' }
