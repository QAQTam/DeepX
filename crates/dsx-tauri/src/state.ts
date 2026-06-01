const _listeners = new Set<() => void>()

export const execLiveOutput: Record<string, string> = {}
export const toolResults: Record<string, { content: string; success: boolean }> = {}

export function clearToolOutputs() {
  for (const k of Object.keys(execLiveOutput)) delete execLiveOutput[k]
  for (const k of Object.keys(toolResults)) delete toolResults[k]
}

export function onToolOutputChange(cb: () => void) {
  _listeners.add(cb)
  return () => { _listeners.delete(cb) }
}

export function notifyToolOutputChange() {
  _listeners.forEach(cb => cb())
}
