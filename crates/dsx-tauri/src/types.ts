export interface Message {
  role: 'user' | 'assistant' | 'tool'
  content: string
  reasoning?: string
  reasoningSegments?: string[]
  tool_calls?: ToolCallEntry[]
  tool_call_id?: string
  name?: string
}

export interface ToolCallEntry {
  id?: string
  name: string
  args: string
  output?: string
}

export interface SessionMeta {
  seed: string
  model?: string
  updated_at?: number
  message_count?: number
  messages?: unknown[]
  last_summary?: string
}