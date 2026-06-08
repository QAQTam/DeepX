// ── useAgentEvents Hook ──
// All Tauri event listening consolidated into a single createStore.
// Replaces 13 individual createSignal calls with one store.

import { createSignal, onMount, onCleanup, type Accessor } from 'solid-js'
import { createStore, produce } from 'solid-js/store'
import { listen } from '@tauri-apps/api/event'
import type { Message, MessageBlock } from '../types'
import type { AgentHandle } from './useAgent'

interface SessionHandle {
  loadMessages: (seed: string, offset?: number, limit?: number) => Promise<{ messages: unknown[]; total: number; offset: number }>
  refresh: () => Promise<void>
}

interface DocsHandle {
  updateFromSnapshot: (payload: { documents?: any[]; recent_edits?: string[]; tasks?: any[] }) => void
}

interface BalanceHandle {
  setBalance: (s: string) => void
}

interface ToastHandle {
  addToast: (message: string, type?: 'info' | 'success' | 'warning' | 'error') => void
}

interface EventsState {
  messages: Message[]
  streamingThink: string
  streamingText: string
  streamingToolNames: string[]
  streamKind: 'thinking' | 'tool_calling' | 'answering' | null
  thinkingSecs: number
  tokenUsage: { used: number; limit: number }
  cacheInfo: { hit: number; miss: number }
  auditLog: Array<{ name: string; args: string; success: boolean }>
  askUser: { question: string; options?: string[] } | null
  askAnswer: string
  liveToolOutputs: Record<string, string>
  msgCount: number
}

export interface AgentEventsHandle {
  readonly messages: Accessor<Message[]>
  readonly streamingThink: Accessor<string>
  readonly streamingText: Accessor<string>
  readonly streamingToolNames: Accessor<string[]>
  readonly streamKind: Accessor<'thinking' | 'tool_calling' | 'answering' | null>
  readonly thinkingSecs: Accessor<number>
  readonly tokenUsage: Accessor<{ used: number; limit: number }>
  readonly cacheInfo: Accessor<{ hit: number; miss: number }>
  readonly auditLog: Accessor<Array<{ name: string; args: string; success: boolean }>>
  readonly askUser: Accessor<{ question: string; options?: string[] } | null>
  readonly askAnswer: Accessor<string>
  readonly liveToolOutputs: Accessor<Record<string, string>>
  readonly msgCount: Accessor<number>
  /** Incremented on in-place message content changes (e.g. tool results) */
  readonly contentVersion: Accessor<number>
  notifyResize: () => void
  setMessages: (fn: (prev: Message[]) => Message[]) => void
  setAskUser: (v: { question: string; options?: string[] } | null) => void
  setAskAnswer: (v: string) => void
  setTokenUsage: (fn: (prev: { used: number; limit: number }) => { used: number; limit: number }) => void
}

const MAX_MESSAGES = 200
const MAX_TOOL_OUTPUT = 8000
const THINKING_TIMER_MAX_SECS = 600

export function useAgentEvents(
  agent: AgentHandle,
  session: SessionHandle,
  docs: DocsHandle,
  balance: BalanceHandle,
  toast: ToastHandle,
): AgentEventsHandle {
  const [state, setState] = createStore<EventsState>({
    messages: [],
    streamingThink: '',
    streamingText: '',
    streamingToolNames: [],
    streamKind: null,
    thinkingSecs: 0,
    tokenUsage: { used: 0, limit: 150000 },
    cacheInfo: { hit: 0, miss: 0 },
    auditLog: [],
    askUser: null,
    askAnswer: '',
    liveToolOutputs: {},
    msgCount: 0,
  })

  // ── Accessor wrappers (read from store, tracked by SolidJS) ──
  const messages = () => state.messages
  const streamingThink = () => state.streamingThink
  const streamingText = () => state.streamingText
  const streamingToolNames = () => state.streamingToolNames
  const streamKind = () => state.streamKind
  const thinkingSecs = () => state.thinkingSecs
  const tokenUsage = () => state.tokenUsage
  const cacheInfo = () => state.cacheInfo
  const auditLog = () => state.auditLog
  const askUser = () => state.askUser
  const askAnswer = () => state.askAnswer
  const liveToolOutputs = () => state.liveToolOutputs
  const msgCount = () => state.msgCount
  const [_contentVersion, setContentVersion] = createSignal(0)
  const contentVersion = () => _contentVersion()

  // ── Setters ──

  const setMessages = (fn: (prev: Message[]) => Message[]) => {
    setState('messages', prev => {
      const next = fn(prev)
      return next.length > MAX_MESSAGES ? next.slice(next.length - MAX_MESSAGES) : next
    })
  }

  const setAskUser = (v: { question: string; options?: string[] } | null) => setState('askUser', v)
  const setAskAnswer = (v: string) => setState('askAnswer', v)

  const setTokenUsage = (fn: (prev: { used: number; limit: number }) => { used: number; limit: number }) => {
    setState('tokenUsage', fn)
  }

  // ── Helpers ──

  const truncateToolOutputs = (msgs: Message[]): Message[] => {
    return msgs.map(msg => {
      if (!msg.blocks) return msg
      const truncated = msg.blocks.map(b => {
        if (b.type !== 'tool' || !b.card.output || b.card.output.length <= MAX_TOOL_OUTPUT) return b
        return { ...b, card: { ...b.card, output: b.card.output.slice(0, MAX_TOOL_OUTPUT) + '\n... [truncated]' } }
      })
      return { ...msg, blocks: truncated }
    })
  }

  const REASONING_RE = /<reasoning>([\s\S]*?)<\/reasoning>/i

  const migrateOldMessage = (msg: any): Message => {
    if (msg.blocks) return msg as Message
    if (msg.role !== 'assistant') return { role: msg.role, content: msg.content || '' }
    const blocks: MessageBlock[] = []
    const match = REASONING_RE.exec(msg.content || '')
    if (match) {
      blocks.push({ type: 'reasoning', content: match[1].trim() })
    } else if (msg.reasoning) {
      blocks.push({ type: 'reasoning', content: msg.reasoning })
    }
    const text = (msg.content || '').replace(REASONING_RE, '').trim()
    if (text) {
      blocks.push({ type: 'text', content: text })
    }
    if (Array.isArray(msg.tool_cards)) {
      for (const tc of msg.tool_cards) {
        blocks.push({ type: 'tool', card: tc })
      }
    }
    return { role: 'assistant', content: msg.content || '', blocks }
  }

  let thinkStart = 0
  let timerRef: ReturnType<typeof setInterval> | null = null
  let safetyRef: ReturnType<typeof setTimeout> | null = null

  const startThinkingTimer = () => {
    thinkStart = Date.now()
    timerRef = setInterval(() => setState('thinkingSecs', Math.floor((Date.now() - thinkStart) / 1000)), 500)
    safetyRef = setTimeout(() => stopThinkingTimer(), THINKING_TIMER_MAX_SECS * 1000)
  }

  const stopThinkingTimer = () => {
    if (timerRef) { clearInterval(timerRef); timerRef = null }
    if (safetyRef) { clearTimeout(safetyRef); safetyRef = null }
    setState('thinkingSecs', 0)
  }

  const pushMsg = (msg: Message) => {
    setMessages(prev => [...prev, msg])
    setState('msgCount', c => c + 1)
  }

  const clearStreaming = () => {
    setState({ streamingThink: '', streamingText: '', streamingToolNames: [], streamKind: null })
  }

  const clearLiveOutputs = () => setState('liveToolOutputs', {})

  // ── Listen for agent-event stream ──
  onMount(() => {
    const unlistens: (() => void)[] = []

    listen('agent-event', (e: { payload: Record<string, unknown> }) => {
      const p = e.payload
      if (!p || typeof p.type !== 'string') return

      switch (p.type) {
        case 'turn_start': {
          agent.dispatch({ type: 'TURN_START', turn_id: p.turn_id as string, user_text: p.user_text as string })
          clearStreaming()
          clearLiveOutputs()
          startThinkingTimer()
          break
        }
        case 'round_delta': {
          const kind = (p.kind as string) || ''
          const delta = (p.delta as string) || ''
          if (kind === 'thinking') {
            setState('streamingThink', prev => prev + delta)
            setState('streamKind', 'thinking')
          } else if (kind === 'answering') {
            setState('streamingText', prev => prev + delta)
            setState('streamKind', 'answering')
          } else if (kind === 'tool_calling') {
            setState('streamingToolNames', prev => prev.includes(delta) ? prev : [...prev, delta])
            setState('streamKind', 'tool_calling')
          }
          break
        }
        case 'round_complete': {
          const rawBlocks = (p.blocks as any[]) || []
          let blocks: MessageBlock[]

          if (rawBlocks.length > 0) {
            blocks = rawBlocks.map((b: any) => {
              if (b.type === 'reasoning') return { type: 'reasoning' as const, content: b.content }
              if (b.type === 'text')      return { type: 'text'      as const, content: b.content }
              if (b.type === 'tool')      return { type: 'tool'      as const, card: b.card }
              return null
            }).filter(Boolean) as MessageBlock[]
          } else {
            // Legacy fallback: flat thinking / answer / tool_calls
            const thinking = (p.thinking as string) || ''
            const answer   = (p.answer   as string) || ''
            const toolCalls = (p.tool_calls as any[]) || []
            blocks = []
            if (thinking) blocks.push({ type: 'reasoning', content: thinking })
            if (answer)   blocks.push({ type: 'text',      content: answer })
            for (const tc of toolCalls) {
              blocks.push({ type: 'tool', card: {
                id: tc.id,
                name: tc.name || '?',
                args: tc.args_display || '',
              }})
            }
          }

          if (blocks.length > 0) {
            pushMsg({ role: 'assistant', content: '', blocks })
          }
          clearStreaming()
          break
        }
        case 'tool_exec_delta': {
          const tid = (p as any).tool_call_id as string
          const delta = (p as any).delta as string
          if (tid && delta) {
            setState('liveToolOutputs', tid, prev => (prev || '') + delta)
          }
          break
        }
        case 'tool_results': {
          const results = (p.results as any[]) || []
          const outputs = state.liveToolOutputs
          setMessages(prev => {
            const msgs = prev.slice()
            for (let i = msgs.length - 1; i >= 0; i--) {
              const msg = msgs[i]
              if (msg.role !== 'assistant' || !msg.blocks) continue
              if (!msg.blocks.some(b => b.type === 'tool')) continue
              msgs[i] = {
                ...msg,
                blocks: msg.blocks.map(b => {
                  if (b.type !== 'tool') return b
                  const match = results.find((r: any) => r.tool_call_id === b.card.id)
                  if (!match) {
                    const live = outputs[b.card.id || '']
                    return live ? { ...b, card: { ...b.card, output: live } } : b
                  }
                  return { ...b, card: { ...b.card, output: match.output, success: match.success } }
                })
              }
              return msgs
            }
            return msgs
          })
          clearLiveOutputs()
          setContentVersion(v => v + 1)
          break
        }
        case 'turn_end': {
          agent.dispatch({ type: 'TURN_END', turn_id: p.turn_id as string })
          stopThinkingTimer()
          clearStreaming()
          clearLiveOutputs()
          const u = (p as any).usage
          if (u) {
            setState('tokenUsage', prev => ({
              used: u.prompt_tokens || prev.used,
              limit: (p as any).context_limit || prev.limit,
            }))
            setState('cacheInfo', { hit: u.prompt_cache_hit_tokens || 0, miss: u.prompt_cache_miss_tokens || 0 })
          }
          break
        }
        case 'audit_record': {
          setState('auditLog', produce((log) => {
            log.push({
              name: (p as any).tool_name as string || '?',
              args: (p as any).result_summary as string || '',
              success: !!(p as any).success,
            })
            if (log.length > 20) log.splice(0, log.length - 20)
          }))
          break
        }
        case 'ask_user': {
          setState('askUser', { question: p.question as string, options: p.options as string[] | undefined })
          break
        }
        case 'balance': {
          const tb = (p as any).total_balance as string
          const cur = (p as any).currency as string
          if (tb && cur) {
            balance.setBalance(`${tb} ${cur}`)
          }
          break
        }
        case 'session_restored': {
          agent.dispatch({ type: 'RESTORE_SESSION', seed: (p.seed as string) || '' })
          const seed = (p as any).seed as string
          setState('tokenUsage', prev => ({ ...prev, used: (p as any).tokens_used || prev.used }))
          if (seed) {
            session.loadMessages(seed).then(({ messages: msgs, total }) => {
              const arr = (Array.isArray(msgs) ? msgs : []).map(migrateOldMessage)
              const truncated = truncateToolOutputs(arr)
              setState('messages', truncated)
              setState('msgCount', truncated.length)
              if (total > truncated.length) {
                toast.addToast(`Loaded ${truncated.length}/${total} messages (older ones hidden)`, 'info')
              }
            }).catch(e => {
              toast.addToast('Load failed: ' + String(e), 'error')
            })
          }
          break
        }
        case 'session_created': {
          const seed = (p as any).seed as string
          if (seed) {
            agent.dispatch({ type: 'RESTORE_SESSION', seed })
            setState('messages', [])
            setState('msgCount', 0)
            setContentVersion(v => v + 1)
          }
          break
        }
        case 'debug_snapshot': {
          docs.updateFromSnapshot({
            documents: (p as any).documents,
            recent_edits: (p as any).recent_edits,
            tasks: (p as any).tasks,
          })
          const hit = (p as any).prompt_cache_hit_tokens
          const miss = (p as any).prompt_cache_miss_tokens
          if (typeof hit === 'number' && typeof miss === 'number') {
            setState('cacheInfo', { hit, miss })
          }
          const ctx = (p as any).context_tokens
          if (typeof ctx === 'number' && ctx > 0) {
            setState('tokenUsage', 'used', ctx)
          }
          break
        }
        case 'error': {
          agent.dispatch({ type: 'ERROR', message: (p.message as string) || 'Agent error' })
          const msg = (p as any).message as string || 'Agent error'
          toast.addToast(msg, 'error')
          pushMsg({ role: 'assistant', content: `\u26a0 ${msg}` })
          break
        }
        case 'tool_notice': {
          const level = (p as any).level as string
          const msg = (p as any).message as string || ''
          if (level === 'warn' || level === 'error') {
            pushMsg({ role: 'system', content: `\u26a0\uFE0F ${msg}` })
          }
          break
        }
        case 'done':
        case 'cancelled': {
          if (p.type === 'cancelled') {
            setState({ askUser: null, askAnswer: '' })
          }
          clearStreaming()
          clearLiveOutputs()
          stopThinkingTimer()
          break
        }
      }
    }).then(fn => unlistens.push(fn))

    onCleanup(() => {
      unlistens.forEach(fn => fn())
      stopThinkingTimer()
    })
  })

  const notifyResize = () => setContentVersion(v => v + 1)

  return {
    messages, streamingThink, streamingText, streamingToolNames,
    streamKind, thinkingSecs, tokenUsage, cacheInfo,
    auditLog, askUser, askAnswer, liveToolOutputs, msgCount, contentVersion,
    setMessages, setAskUser, setAskAnswer, setTokenUsage, notifyResize,
  }
}
