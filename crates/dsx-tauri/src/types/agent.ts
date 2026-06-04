// ── Agent Event Types (14 union types) ──

export type StreamKind = 'thinking' | 'tool_calling' | 'answering'

export interface StreamStartPayload {
  type: 'stream_start'
  kind: StreamKind
  tool_names?: string[]
}

export interface StreamDeltaPayload {
  type: 'stream_delta'
  delta: string
  kind?: 'thinking' | 'content'
}

export interface StreamEndPayload {
  type: 'stream_end'
  is_final: boolean
}

export interface UsageUpdatePayload {
  type: 'usage_update'
  prompt_tokens?: number
  context_limit?: number
  cache_hit_tokens?: number
  cache_miss_tokens?: number
}

export interface AssistantMsgPayload {
  type: 'assistant_msg'
  thinking?: string
  text: string
}

export interface ToolCallPayload {
  type: 'tool_call'
  tool?: { id: string; name: string; args_display?: string; body?: unknown }
  id?: string
  name?: string
  args_display?: string
  body?: unknown
}

export interface UserMsgPayload {
  type: 'user_msg'
  id: string
  text: string
}

export interface ToolResultPayload {
  type: 'tool_result'
  tool_id: string
  output: string
  success?: boolean
}

export interface TurnEndPayload {
  type: 'turn_end'
  usage?: {
    prompt_tokens: number
    prompt_cache_hit_tokens?: number
    prompt_cache_miss_tokens?: number
  }
  context_limit?: number
  stop_reason?: string
}

export interface DonePayload {
  type: 'done'
}

export interface ErrorPayload {
  type: 'error'
  message?: string
}

export interface CancelledPayload {
  type: 'cancelled'
}

export interface AskUserPayload {
  type: 'ask_user'
  question: string
  options?: string[]
}

export interface BalancePayload {
  type: 'balance'
  total_balance: string
  currency: string
}

export interface SessionRestoredPayload {
  type: 'session_restored'
  seed: string
  tokens_used?: number
  cache_hit_pct?: number
}

export interface DebugSnapshotPayload {
  type: 'debug_snapshot'
  context_tokens?: number
  context_limit?: number
  documents?: Array<{ tag: string; path: string; turns_since_read: number; is_stale: boolean }>
  recent_edits?: string[]
  tasks?: Array<{ subject: string; description: string; status: string }>
  dsml_compat_count?: number
}

export interface AuditRecordPayload {
  type: 'audit_record'
  [key: string]: unknown
}

export interface ShutdownAckPayload {
  type: 'shutdown_ack'
}

/** Discriminated union of all agent events */
export type AgentEvent =
  | StreamStartPayload
  | StreamDeltaPayload
  | StreamEndPayload
  | UsageUpdatePayload
  | AssistantMsgPayload
  | ToolCallPayload
  | UserMsgPayload
  | ToolResultPayload
  | TurnEndPayload
  | DonePayload
  | ErrorPayload
  | CancelledPayload
  | AskUserPayload
  | BalancePayload
  | SessionRestoredPayload
  | DebugSnapshotPayload
  | AuditRecordPayload
  | ShutdownAckPayload

export const STOP_REASON_LABELS: Record<string, string> = {
  length: 'stopLength',
  content_filter: 'stopContentFilter',
  insufficient_system_resource: 'stopResource',
  error: 'stopError',
} as const
