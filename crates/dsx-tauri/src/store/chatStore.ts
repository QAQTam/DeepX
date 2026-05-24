import { create } from 'zustand'
import type { Message } from '../types'

interface ChatState {
  messages: Message[]
  setMessages: (msgs: Message[] | ((prev: Message[]) => Message[])) => void
  isStreaming: boolean
  setIsStreaming: (v: boolean) => void
  streamMode: 'idle' | 'think' | 'answer'
  setStreamMode: (v: 'idle' | 'think' | 'answer') => void
  sessionId: string
  setSessionId: (v: string) => void
  currentPhase: string
  setCurrentPhase: (v: string) => void
  planVersion: number
  bumpPlanVersion: () => void
}

export const useChatStore = create<ChatState>((set) => ({
  messages: [],
  setMessages: (msgs) =>
    set((s) => ({
      messages: typeof msgs === 'function' ? msgs(s.messages) : msgs,
    })),
  isStreaming: false,
  setIsStreaming: (v) => set({ isStreaming: v }),
  streamMode: 'idle',
  setStreamMode: (v) => set({ streamMode: v }),
  sessionId: '',
  setSessionId: (v) => set({ sessionId: v }),
  currentPhase: 'coding',
  setCurrentPhase: (v) => set({ currentPhase: v }),
  planVersion: 0,
  bumpPlanVersion: () => set((s) => ({ planVersion: s.planVersion + 1 })),
}))
