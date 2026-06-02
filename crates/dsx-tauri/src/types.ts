export interface Message {
  role: 'user' | 'assistant'
  content: string
  reasoning?: string
  tool_cards?: ToolCardEntry[]
}

export interface ToolCardEntry {
  id?: string
  name: string
  args: string
  body?: any
  output?: string
  success?: boolean
}
