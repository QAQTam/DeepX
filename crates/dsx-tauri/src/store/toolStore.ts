import { create } from 'zustand'

interface ToolStore {
  execLiveOutput: Record<string, string>
  toolResults: Record<string, { content: string; success: boolean }>
  appendExecOutput: (id: string, line: string) => void
  setToolResult: (id: string, name: string, content: string, success: boolean) => void
  clearToolOutputs: () => void
}

export const useToolStore = create<ToolStore>((set) => ({
  execLiveOutput: {},
  toolResults: {},
  appendExecOutput: (id, line) =>
    set((s) => ({
      execLiveOutput: {
        ...s.execLiveOutput,
        [id]: (s.execLiveOutput[id] || '') + line + '\n',
      },
    })),
  setToolResult: (id, name, content, success) =>
    set((s) => ({
      toolResults: {
        ...s.toolResults,
        [id]: { content, success },
        [name]: { content, success },
      },
    })),
  clearToolOutputs: () => set({ execLiveOutput: {}, toolResults: {} }),
}))
