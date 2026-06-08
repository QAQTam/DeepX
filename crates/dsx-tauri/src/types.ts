export type MessageBlock =
  | { type: 'reasoning'; content: string }
  | { type: 'text';      content: string }
  | { type: 'tool';      card: ToolCardEntry }

export interface Message {
  role: 'user' | 'assistant' | 'system'
  content: string
  blocks?: MessageBlock[]
}

export interface ToolCardEntry {
  id?: string
  name: string
  args: string
  body?: any
  output?: string
  liveOutput?: string
  success?: boolean
}

export interface DocInfo {
  tag: string
  path: string
  turns_since_read: number
  is_stale: boolean
}
