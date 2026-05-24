import { create } from 'zustand'

interface UIState {
  showSettings: boolean
  setShowSettings: (v: boolean) => void
  askUser: { question: string; options?: string[] } | null
  setAskUser: (v: { question: string; options?: string[] } | null) => void
  toolConfirm: { id: string; toolName: string; action: string; prompt: string } | null
  setToolConfirm: (v: { id: string; toolName: string; action: string; prompt: string } | null) => void
  toolState: unknown
  setToolState: (v: unknown) => void
  askAnswer: string
  setAskAnswer: (v: string) => void
  configVersion: number
  bumpConfigVersion: () => void
}

export const useUIStore = create<UIState>((set) => ({
  showSettings: false,
  setShowSettings: (v) => set({ showSettings: v }),
  askUser: null,
  setAskUser: (v) => set({ askUser: v }),
  toolConfirm: null,
  setToolConfirm: (v) => set({ toolConfirm: v }),
  toolState: null,
  setToolState: (v) => set({ toolState: v }),
  askAnswer: '',
  setAskAnswer: (v) => set({ askAnswer: v }),
  configVersion: 0,
  bumpConfigVersion: () => set((s) => ({ configVersion: s.configVersion + 1 })),
}))
