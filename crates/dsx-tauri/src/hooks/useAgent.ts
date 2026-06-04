// ── useAgent Hook ──
// Manages agent lifecycle: start → connect → stream → done.
// Extracts 150+ lines from App.tsx.

import { useReducer, useCallback, useRef, useEffect } from 'react'
import { listen } from '@tauri-apps/api/event'
import { api } from '../bridge/tauri'
import {
  agentReducer,
  createInitialState,
  type AgentState,
  type AgentAction,
  type ToolCardEntry,
} from '../domain/agent.fsm'

export interface AgentHandle {
  state: AgentState
  start: () => Promise<void>
  stop: () => Promise<void>
  resume: (seed: string) => Promise<void>
  cancel: () => void
  send: (text: string) => void
  dispatch: (action: AgentAction) => void
  isStreaming: boolean
  streamContent: string
  streamReasoning: string
  streamToolCards: ToolCardEntry[]
}

export function useAgent(): AgentHandle {
  const [state, dispatch] = useReducer(agentReducer, null, createInitialState)
  const restartRef = useRef(0)

  // ── Event stream listener ──
  useEffect(() => {
    const unlistens: Promise<() => void>[] = []
    const gen = ++restartRef.current

    const on = (event: string, handler: (e: any) => void) => {
      unlistens.push(listen(event, handler).then(fn => fn))
    }

    on('agent-event', (e: { payload: Record<string, unknown> }) => {
      if (gen !== restartRef.current) return
      const p = e.payload
      if (!p || typeof p.type !== 'string') return

      switch (p.type) {
        case 'stream_start':
          dispatch({ type: 'STREAM_START', kind: (p.kind as any) || 'answering', toolNames: (p.tool_names as string[]) || [] })
          break
        case 'stream_delta':
          dispatch({ type: 'STREAM_DELTA', delta: p.delta as string })
          break
        case 'stream_end':
          dispatch({ type: 'STREAM_END', isFinal: !!p.is_final })
          break
        case 'done':
          dispatch({ type: 'FLUSH_STREAM' })
          break
        case 'error':
          dispatch({ type: 'ERROR', message: (p.message as string) || 'Agent error' })
          break
        case 'cancelled':
          dispatch({ type: 'CANCEL' })
          break
        case 'shutdown_ack':
          dispatch({ type: 'SHUTDOWN_ACK' })
          break
        case 'session_restored':
          dispatch({ type: 'RESTORE_SESSION', seed: (p.seed as string) || '' })
          break
      }
    })

    on('agent-closed', () => {
      if (gen === restartRef.current) dispatch({ type: 'AGENT_CLOSED' })
    })

    return () => {
      unlistens.forEach(p => p.then(fn => fn()).catch(() => {}))
    }
  }, [])

  // ── Check if agent already running (page refresh recovery) ──
  useEffect(() => {
    api.checkAgentStatus().then(s => {
      if (s.running) {
        dispatch({ type: 'CONNECTED', seed: s.seed || '', sessions: [] })
      }
    }).catch(() => {})
  }, [])

  // ── Commands ──
  const start = useCallback(async () => {
    dispatch({ type: 'START_CONNECT' })
    try {
      const res = await api.startAgent()
      dispatch({ type: 'CONNECTED', seed: res.seed || '', sessions: res.sessions || [] })
    } catch (e: any) {
      dispatch({ type: 'ERROR', message: String(e) })
    }
  }, [])

  const stop = useCallback(async () => {
    try {
      await api.stopAgent()
    } catch { /* ignore */ }
    dispatch({ type: 'DISCONNECT' })
  }, [])

  const resume = useCallback(async (seed: string) => {
    dispatch({ type: 'START_CONNECT' })
    try {
      await api.resumeAgent(seed)
      dispatch({ type: 'CONNECTED', seed, sessions: state.sessions })
    } catch (e: any) {
      dispatch({ type: 'ERROR', message: String(e) })
    }
  }, [state.sessions])

  const cancel = useCallback(() => {
    api.cancelAgent().catch(() => {})
    dispatch({ type: 'CANCEL' })
  }, [])

  const send = useCallback((text: string) => {
    api.sendMessage(text).catch(() => {})
  }, [])

  return {
    state,
    start, stop, resume, cancel, send, dispatch,
    isStreaming: state.status === 'streaming',
    streamContent: state.stream.content,
    streamReasoning: state.stream.reasoning,
    streamToolCards: state.stream.toolCards,
  }
}
