// ── useAgentEvents Hook ──
// Extracts all Tauri event listening from App.tsx.
// Handles: agent-event stream, streaming state, message accumulation.

import { createSignal, onMount, onCleanup, type Accessor } from 'solid-js'
import { listen } from '@tauri-apps/api/event'
import type { Message } from '../types'
import type { AgentHandle } from './useAgent'

interface SessionHandle {
  loadMessages: (seed: string) => Promise<unknown[]>
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
  setMessages: (fn: (prev: Message[]) => Message[]) => void
  setAskUser: (v: { question: string; options?: string[] } | null) => void
  setAskAnswer: (v: string) => void
  setTokenUsage: (fn: (prev: { used: number; limit: number }) => { used: number; limit: number }) => void
}

export function useAgentEvents(
  agent: AgentHandle,
  session: SessionHandle,
  docs: DocsHandle,
  balance: BalanceHandle,
  toast: ToastHandle,
): AgentEventsHandle {
  const [messages, setMessages] = createSignal<Message[]>([])
  const [thinkingSecs, setThinkingSecs] = createSignal(0)
  const [tokenUsage, setTokenUsage] = createSignal({ used: 0, limit: 150000 })
  const [cacheInfo, setCacheInfo] = createSignal({ hit: 0, miss: 0 })
  const [auditLog, setAuditLog] = createSignal<Array<{ name: string; args: string; success: boolean }>>([])
  const [streamingThink, setStreamingThink] = createSignal('')
  const [streamingText, setStreamingText] = createSignal('')
  const [streamingToolNames, setStreamingToolNames] = createSignal<string[]>([])
  const [streamKind, setStreamKind] = createSignal<'thinking' | 'tool_calling' | 'answering' | null>(null)
  const [askUser, setAskUser] = createSignal<{ question: string; options?: string[] } | null>(null)
  const [askAnswer, setAskAnswer] = createSignal('')

  let thinkStart = 0
  let timerRef: ReturnType<typeof setInterval> | null = null

  // ── Thinking timer ──
  const startThinkingTimer = () => {
    thinkStart = Date.now()
    timerRef = setInterval(() => setThinkingSecs(Math.floor((Date.now() - thinkStart) / 1000)), 200)
  }

  const stopThinkingTimer = () => {
    if (timerRef) { clearInterval(timerRef); timerRef = null }
    setThinkingSecs(0)
  }

  // ── Helper: push message ──
  const pushMsg = (msg: Message) => setMessages(prev => [...prev, msg])

  // ── Listen for agent-event stream ──
  onMount(() => {
    const unlistens: (() => void)[] = []

    listen('agent-event', (e: { payload: Record<string, unknown> }) => {
      const p = e.payload
      if (!p || typeof p.type !== 'string') return

      switch (p.type) {
        case 'turn_start': {
          agent.dispatch({ type: 'TURN_START', turn_id: p.turn_id as string, user_text: p.user_text as string })
          setStreamingThink('')
          setStreamingText('')
          setStreamingToolNames([])
          setStreamKind(null)
          startThinkingTimer()
          break
        }
        case 'round_delta': {
          const kind = (p.kind as string) || ''
          const delta = (p.delta as string) || ''
          if (kind === 'thinking') {
            setStreamingThink(prev => prev + delta)
            setStreamKind('thinking')
          } else if (kind === 'answering') {
            setStreamingText(prev => prev + delta)
            setStreamKind('answering')
          } else if (kind === 'tool_calling') {
            setStreamingToolNames(prev => prev.includes(delta) ? prev : [...prev, delta])
            setStreamKind('tool_calling')
          }
          break
        }
        case 'round_complete': {
          const thinking = (p.thinking as string) || ''
          const answer = (p.answer as string) || ''
          const toolCalls = (p.tool_calls as any[]) || []
          const combined = (thinking ? `\n<reasoning>${thinking}</reasoning>\n` : '') + answer
          if (combined.trim()) {
            pushMsg({
              role: 'assistant',
              content: combined,
              reasoning: thinking || undefined,
              tool_cards: toolCalls.map((tc: any) => ({
                id: tc.id,
                name: tc.name || '?',
                args: tc.args_display || '',
              })),
            })
          } else if (toolCalls.length > 0) {
            pushMsg({
              role: 'assistant',
              content: toolCalls.map((tc: any) => tc.name || '?').join(', '),
              tool_cards: toolCalls.map((tc: any) => ({
                id: tc.id,
                name: tc.name || '?',
                args: tc.args_display || '',
              })),
            })
          }
          setStreamingThink('')
          setStreamingText('')
          setStreamingToolNames([])
          setStreamKind(null)
          break
        }
        case 'tool_exec_delta': {
          const tid = (p as any).tool_call_id as string
          const delta = (p as any).delta as string
          if (tid && delta) {
            setMessages(prev => {
              for (let i = prev.length - 1; i >= 0; i--) {
                const tc = prev[i].tool_cards
                if (tc) {
                  const idx = tc.findIndex(c => c.id === tid)
                  if (idx >= 0) {
                    const msgs = prev.slice()
                    const cards = msgs[i].tool_cards!.slice()
                    cards[idx] = { ...cards[idx], liveOutput: (cards[idx].liveOutput || '') + delta }
                    msgs[i] = { ...msgs[i], tool_cards: cards }
                    return msgs
                  }
                }
              }
              return prev
            })
          }
          break
        }
        case 'tool_results': {
          const results = (p.results as any[]) || []
          setMessages(prev => {
            const msgs = [...prev]
            for (let i = msgs.length - 1; i >= 0; i--) {
              if (msgs[i].role === 'assistant' && msgs[i].tool_cards) {
                const cards = msgs[i].tool_cards!.map(tc => {
                  const match = results.find((r: any) => r.tool_call_id === tc.id)
                  return match ? { ...tc, output: match.output, success: match.success } : tc
                })
                msgs[i] = { ...msgs[i], tool_cards: cards }
                return msgs
              }
            }
            return msgs
          })
          break
        }
        case 'turn_end': {
          agent.dispatch({ type: 'TURN_END', turn_id: p.turn_id as string })
          stopThinkingTimer()
          setStreamingThink('')
          setStreamingText('')
          setStreamingToolNames([])
          setStreamKind(null)
          const u = (p as any).usage
          if (u) {
            setTokenUsage(prev => ({
              used: u.prompt_tokens || prev.used,
              limit: (p as any).context_limit || prev.limit,
            }))
            setCacheInfo({ hit: u.prompt_cache_hit_tokens || 0, miss: u.prompt_cache_miss_tokens || 0 })
          }
          break
        }
        case 'audit_record': {
          setAuditLog(prev => [...prev, {
            name: (p as any).tool_name as string || '?',
            args: (p as any).result_summary as string || '',
            success: !!(p as any).success,
          }].slice(-20))
          break
        }
        case 'ask_user': {
          setAskUser({ question: p.question as string, options: p.options as string[] | undefined })
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
          agent.dispatch({ type: 'RESTORE_SESSION', seed: (p.seed as string) || '', turns: (p.turns as any[]) || [] })
          const seed = (p as any).seed as string
          setTokenUsage(prev => ({ ...prev, used: (p as any).tokens_used || prev.used }))
          if (seed) {
            session.loadMessages(seed).then(msgs => {
              const arr = (Array.isArray(msgs) ? msgs : (msgs as any)?.messages ?? []) as Message[]
              setMessages(arr)
            }).catch(e => {
              toast.addToast('Load failed: ' + String(e), 'error')
            })
          }
          break
        }
        case 'session_created': {
          const seed = (p as any).seed as string
          if (seed) {
            agent.dispatch({ type: 'RESTORE_SESSION', seed, turns: [] })
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
            setCacheInfo({ hit, miss })
          }
          const ctx = (p as any).context_tokens
          if (typeof ctx === 'number' && ctx > 0) {
            setTokenUsage(prev => ({ ...prev, used: ctx }))
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
            setAskUser(null)
            setAskAnswer('')
          }
          break
        }
      }
    }).then(fn => unlistens.push(fn))

    onCleanup(() => {
      unlistens.forEach(fn => fn())
      stopThinkingTimer()
    })
  })

  return {
    messages,
    streamingThink,
    streamingText,
    streamingToolNames,
    streamKind,
    thinkingSecs,
    tokenUsage,
    cacheInfo,
    auditLog,
    askUser,
    askAnswer,
    setMessages,
    setAskUser,
    setAskAnswer,
    setTokenUsage,
  }
}
