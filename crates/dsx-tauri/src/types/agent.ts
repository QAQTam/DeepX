// ── Agent data types (session restore + FSM) ──

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
