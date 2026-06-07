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
  createSession: () => Promise<void>
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
    // Listen for agent-closed (connection lost)
    const unlistens: (() => void)[] = []
    const currentGen = ++gen

    listen('agent-closed', () => {
      if (currentGen === gen) dispatch({ type: 'AGENT_CLOSED' })
    }).then(fn => unlistens.push(fn))

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
    }).catch(e => console.error('checkAgentStatus failed:', e)).finally(() => setStatusChecked(true))
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

  const createSession = async () => {
    try {
      await api.createSession()
    } catch (e: any) {
      dispatch({ type: 'ERROR', message: String(e) })
    }
  }

  const cancel = () => {
    api.cancelAgent().catch(e => console.error('cancelAgent failed:', e))
    dispatch({ type: 'CANCEL' })
  }

  const send = (text: string) => {
    api.sendMessage(text).catch(e => console.error('sendMessage failed:', e))
  }

  return {
    state,
    get isStreaming() { return state.streaming },
    get statusChecked() { return _statusChecked() },
    start, stop, resume, createSession, cancel, send, dispatch,
  }
}
