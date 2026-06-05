// ── useAgent Hook (SolidJS, v5 round-based) ──

import { createStore } from 'solid-js/store'
import { createSignal, onMount, onCleanup } from 'solid-js'
import { listen } from '@tauri-apps/api/event'
import { api } from '../bridge/tauri'
import {
  agentReducer,
  createInitialState,
  type AgentState,
  type AgentAction,
} from '../domain/agent.fsm'

export interface AgentHandle {
  readonly state: AgentState
  readonly isStreaming: boolean
  readonly statusChecked: boolean
  start: () => Promise<void>
  stop: () => Promise<void>
  resume: (seed: string) => Promise<void>
  cancel: () => void
  send: (text: string) => void
  dispatch: (action: AgentAction) => void
}

export function useAgent(): AgentHandle {
  const [state, setState] = createStore(createInitialState())
  const [_statusChecked, setStatusChecked] = createSignal(false)
  let gen = 0

  const dispatch = (action: AgentAction) => {
    setState(agentReducer(state, action))
  }

  onMount(() => {
    const unlistens: (() => void)[] = []
    const currentGen = ++gen

    const on = (event: string, handler: (e: any) => void) => {
      listen(event, handler).then(fn => unlistens.push(fn))
    }

    on('agent-event', (e: { payload: Record<string, unknown> }) => {
      if (currentGen !== gen) return
      const p = e.payload
      if (!p || typeof p.type !== 'string') return

      switch (p.type) {
        case 'turn_start':
          dispatch({ type: 'TURN_START', turn_id: p.turn_id as string, user_text: p.user_text as string })
          break
        case 'round_complete':
          dispatch({
            type: 'ROUND_COMPLETE',
            turn_id: p.turn_id as string,
            round_num: (p.round_num as number) || 0,
            thinking: p.thinking as string | undefined,
            answer: p.answer as string | undefined,
            tool_calls: p.tool_calls as any[] | undefined,
            is_final: !!(p.is_final),
          })
          break
        case 'tool_results':
          dispatch({
            type: 'TOOL_RESULTS',
            turn_id: p.turn_id as string,
            round_num: (p.round_num as number) || 0,
            results: (p.results as any[]) || [],
          })
          break
        case 'turn_end':
          dispatch({ type: 'TURN_END', turn_id: p.turn_id as string })
          break
        case 'done':
          // No-op: streaming already ended by turn_end/round_complete
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
          dispatch({
            type: 'RESTORE_SESSION',
            seed: (p.seed as string) || '',
            turns: (p.turns as any[]) || [],
          })
          break
      }
    })

    on('agent-closed', () => {
      if (currentGen === gen) dispatch({ type: 'AGENT_CLOSED' })
    })

    onCleanup(() => {
      unlistens.forEach(fn => fn())
    })
  })

  // ── Check if agent already running ──
  onMount(() => {
    api.checkAgentStatus().then(s => {
      if (s.running) {
        dispatch({ type: 'CONNECTED', seed: s.seed || '', sessions: [] })
      }
    }).catch(() => {}).finally(() => setStatusChecked(true))
  })

  const start = async () => {
    dispatch({ type: 'START_CONNECT' })
    try {
      const res = await api.startAgent()
      dispatch({ type: 'CONNECTED', seed: res.seed || '', sessions: res.sessions || [] })
    } catch (e: any) {
      dispatch({ type: 'ERROR', message: String(e) })
    }
  }

  const stop = async () => {
    try { await api.stopAgent() } catch { /* ignore */ }
    dispatch({ type: 'DISCONNECT' })
  }

  const resume = async (seed: string) => {
    try {
      await api.resumeAgent(seed)
      dispatch({ type: 'CONNECTED', seed, sessions: state.sessions })
    } catch (e: any) {
      dispatch({ type: 'ERROR', message: String(e) })
    }
  }

  const cancel = () => {
    api.cancelAgent().catch(() => {})
    dispatch({ type: 'CANCEL' })
  }

  const send = (text: string) => {
    api.sendMessage(text).catch(() => {})
  }

  return {
    state,
    get isStreaming() { return state.streaming },
    get statusChecked() { return _statusChecked() },
    start, stop, resume, cancel, send, dispatch,
  }
}
