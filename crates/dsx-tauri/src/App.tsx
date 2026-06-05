// ── App Shell (SolidJS) ──
// State management delegated to hooks. View composition only.
// Migrated from React — all event handlers match original exactly.

import { createSignal, createEffect, onMount, onCleanup, Show } from 'solid-js'
import { listen } from '@tauri-apps/api/event'
import { useAgent, useConfig, useSession, useBalance, useDocuments } from './hooks'
import type { Message } from './types'
import { ChatMessage } from './components/ChatMessage'
import { InfoPanel } from './components/InfoPanel'
import { WorkspacePanel } from './components/WorkspacePanel'
import { StreamIndicator } from './components/StreamIndicator'
import { ConfigWizard } from './components/ConfigWizard'
import { SettingsDialog } from './components/SettingsDialog'
import { AskUserDialog } from './components/AskUserDialog'
import { ReasoningBlock } from './components/chat/ReasoningBlock'
import { useToast } from './components/shared'
import { tt } from './i18n'

export default function App() {
  // ── Hooks (DO NOT destructure — getters must be called in tracking context) ──
  const cfg = useConfig()
  const agent = useAgent()
  const session = useSession()
  const balance = useBalance()
  const docs = useDocuments()
  const toast = useToast()

  // ── UI state ──
  const [messages, setMessages] = createSignal<Message[]>([])
  const [thinkingSecs, setThinkingSecs] = createSignal(0)
  const [tokenUsage, setTokenUsage] = createSignal({ used: 0, limit: 150000 })
  const [cacheInfo, setCacheInfo] = createSignal({ hit: 0, miss: 0 })
  const [showSettings, setShowSettings] = createSignal(false)
  const [leftOpen, setLeftOpen] = createSignal(true)
  const [rightOpen, setRightOpen] = createSignal(true)
  const [askUser, setAskUser] = createSignal<{ question: string; options?: string[] } | null>(null)
  const [askAnswer, setAskAnswer] = createSignal('')
  const [auditLog, setAuditLog] = createSignal<Array<{ name: string; args: string; success: boolean }>>([])
  const [streamingThink, setStreamingThink] = createSignal('')
  const [streamingText, setStreamingText] = createSignal('')
  const [streamingToolNames, setStreamingToolNames] = createSignal<string[]>([])
  const [streamKind, setStreamKind] = createSignal<'thinking' | 'tool_calling' | 'answering' | null>(null)
  const [, setConfigVersion] = createSignal(0)

  let inputRef!: HTMLTextAreaElement
  let msgEndRef!: HTMLDivElement
  let thinkStart = 0
  let timerRef: ReturnType<typeof setInterval> | null = null

  // ── Helper: push message ──
  const pushMsg = (msg: Message) => setMessages(prev => [...prev, msg])

  // ── Helper: token counting ──
  const addTokens = (text: string) => {
    const count = Math.ceil(text.length / 3.5)
    setTokenUsage(prev => ({ ...prev, used: Math.min(prev.used + count, prev.limit) }))
  }

  // ── Sync context limit from loaded config ──
  createEffect(() => {
    const c = cfg.config
    if (c?.context_limit) {
      setTokenUsage(prev => prev.limit === c.context_limit ? prev : { ...prev, limit: c.context_limit! })
    }
  })

  // ── Auto-start agent on launch ──
  createEffect(() => {
    if (cfg.checkDone && cfg.config && agent.statusChecked && !agent.state.connected && agent.state.status === 'idle') {
      agent.start()
    }
  })

  // ── Thinking timer ──
  createEffect(() => {
    if (!agent.state.streaming) {
      setThinkingSecs(0)
      setStreamingThink('')
      setStreamingText('')
      setStreamingToolNames([])
      setStreamKind(null)
      return
    }
    thinkStart = Date.now()
    timerRef = setInterval(() => setThinkingSecs(Math.floor((Date.now() - thinkStart) / 1000)), 200)
    onCleanup(() => {
      if (timerRef) { clearInterval(timerRef); timerRef = null }
      setThinkingSecs(0)
    })
  })

  // ── Refresh session list on connect ──
  createEffect(() => {
    if (agent.state.connected) session.refresh()
  })

  // ── Auto-scroll chat to bottom ──
  createEffect(() => {
    messages()
    agent.state.streaming
    msgEndRef?.scrollIntoView({ behavior: 'instant' })
  })

  // ── Auto-focus input on connect ──
  createEffect(() => {
    if (agent.state.connected) inputRef?.focus()
  })

  // ── Event handlers — matches original React App.tsx exactly ──
  onMount(() => {
    const unlistens: (() => void)[] = []
    const on = (event: string, handler: (e: any) => void) => {
      listen(event, handler).then(fn => unlistens.push(fn))
    }

    on('agent-event', (e: { payload: Record<string, unknown> }) => {
      const p = e.payload
      if (!p || typeof p.type !== 'string') return

      switch (p.type) {
        case 'assistant_msg': {
          const thinking = (p.thinking || '') as string
          const text = (p.text || '') as string
          const content = thinking ? `\n<reasoning>${thinking}</reasoning>\n${text}` : text
          pushMsg({ role: 'assistant', content })
          addTokens(content)
          break
        }
        case 'tool_call': {
          const tool = ((p as any).tool || p) as any
          const toolId = (tool.id as string) || `tc-${Date.now()}`
          const name = (tool.name as string) || 'unknown'
          const argsDisplay = (tool.args_display as string) || ''
          const body = tool.body
          setMessages(prev => {
            const msgs = [...prev]
            for (let i = msgs.length - 1; i >= 0; i--) {
              if (msgs[i].role === 'assistant') {
                const cards = [...(msgs[i].tool_cards || [])]
                cards.push({ id: toolId, name, args: argsDisplay, body })
                msgs[i] = { ...msgs[i], tool_cards: cards }
                return msgs
              }
            }
            return msgs
          })
          break
        }
        case 'tool_result': {
          const toolId = (p.tool_id as string) || ''
          const output = (p.output as string) || ''
          const success = p.success as boolean | undefined
          setMessages(prev => {
            const msgs = [...prev]
            for (let i = msgs.length - 1; i >= 0; i--) {
              if (msgs[i].role === 'assistant' && msgs[i].tool_cards) {
                const cards = msgs[i].tool_cards!.map(tc =>
                  tc.id === toolId ? { ...tc, output, success } : tc
                )
                msgs[i] = { ...msgs[i], tool_cards: cards }
                return msgs
              }
            }
            return msgs
          })
          break
        }
        case 'turn_start': {
          setStreamingThink('')
          setStreamingText('')
          setStreamingToolNames([])
          setStreamKind(null)
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
            addTokens(thinking + answer)
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
      }
    })

    onCleanup(() => unlistens.forEach(fn => fn()))
  })

  // ── Send message ──
  const send = () => {
    const text = inputRef?.value?.trim()
    if (!text || agent.isStreaming || !agent.state.connected) return
    inputRef.value = ''
    pushMsg({ role: 'user', content: text })
    addTokens(text)
    agent.send(text)
    setTimeout(() => inputRef?.focus(), 50)
  }

  // ── Ask answer submit ──
  const submitAskAnswer = () => {
    if (!askUser()) return
    const response = (askAnswer() || '').trim() || 'skipped'
    agent.send(response)
    setAskUser(null)
    setAskAnswer('')
  }

  // ── Loading ──
  return (
    <Show when={!cfg.loading} fallback={
      <div class="h-screen flex items-center justify-center bg-[var(--bg-primary)]">
        <div class="flex flex-col items-center gap-4">
          <div class="w-10 h-10 rounded-full bg-[var(--accent)]/15 flex items-center justify-center anim-pulse-glow">
            <svg width="22" height="22" viewBox="0 0 32 32" fill="none" class="text-[var(--accent)]">
              <path d="M6 4h8l6 10-6 14H6l6-14L6 4z" fill="currentColor" opacity="0.8"/>
              <path d="M18 4h8l-6 14 6 14h-8l-6-14 6-14z" fill="currentColor"/>
            </svg>
          </div>
          <div class="text-sm text-[var(--muted)]">{tt('common.loading')}</div>
        </div>
      </div>
    }>
      {/* Config wizard (first run) */}
      <Show when={!cfg.checkDone}>
        <ConfigWizard onDone={() => { setConfigVersion(v => v + 1); window.location.reload() }} />
      </Show>

      {/* Main App */}
      <Show when={cfg.checkDone}>
        <div class="h-screen flex flex-col bg-[var(--bg-primary)] text-[var(--text)] overflow-hidden">
          {/* Top Bar */}
          <div class="h-10 flex items-center justify-between px-3 border-b border-[var(--border)] bg-[var(--bg-secondary)] shrink-0 transition-theme">
            <div class="flex items-center gap-2">
              <button onClick={() => setLeftOpen(o => !o)} class="w-7 h-7 rounded-md flex items-center justify-center hover:bg-[var(--bg-tertiary)] transition-colors" aria-label="Toggle left panel">
                <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor"><rect x="2" y="3" width="12" height="1.5" rx="0.5"/><rect x="2" y="7" width="12" height="1.5" rx="0.5"/><rect x="2" y="11" width="12" height="1.5" rx="0.5"/></svg>
              </button>
              <span class={`inline-block w-2 h-2 rounded-full ${agent.state.connected ? 'bg-[var(--success)]' : 'bg-[var(--muted)]'}`} />
              <span class="text-xs text-[var(--text)] font-medium">{agent.state.connected ? tt('chat.connected') : tt('chat.disconnected')}</span>
            </div>
            <div class="flex items-center gap-3">
              <div class="text-xs text-[var(--muted)]">上下文 {tokenUsage().used} / {tokenUsage().limit}</div>
              <Show when={agent.isStreaming}>
                <button onClick={() => agent.cancel()} class="text-xs text-[var(--error)] hover:underline">停止</button>
              </Show>
              <button onClick={() => setRightOpen(o => !o)} class="w-7 h-7 rounded-md flex items-center justify-center hover:bg-[var(--bg-tertiary)] transition-colors" aria-label="Toggle right panel">
                <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor"><rect x="2" y="3" width="12" height="1.5" rx="0.5"/><rect x="2" y="7" width="12" height="1.5" rx="0.5"/><rect x="2" y="11" width="12" height="1.5" rx="0.5"/></svg>
              </button>
            </div>
          </div>

          {/* Main Content */}
          <div class="flex-1 flex overflow-hidden">
            {/* Left Panel */}
            <div class={`shrink-0 border-r border-[var(--border)] transition-all duration-200 overflow-hidden ${leftOpen() ? 'w-56' : 'w-0'}`}>
              <div class="w-56 h-full overflow-y-auto p-3 space-y-3">
                <InfoPanel
                  tokens={tokenUsage}
                  cache={cacheInfo}
                  balance={balance.balance}
                  sessionId={() => agent.state.sessionId || ''}
                  sessions={() => session.sessions}
                  auditLog={auditLog()}
                  toolBatch={null}
                  toolNames={streamingToolNames()}
                  onSettings={() => setShowSettings(true)}
                  onNewSession={() => agent.start()}
                  onResumeSession={seed => agent.resume(seed)}
                  onDeleteAllSessions={session.deleteAll}
                  onDeleteSession={seed => session.deleteSession(seed)}
                  onRefreshBalance={() => cfg.config?.api_key && balance.refresh(cfg.config.api_key)}
                />
              </div>
            </div>

            {/* Center Chat */}
            <div class="flex-1 flex flex-col min-w-0">
              {/* Messages */}
              <div class="flex-1 overflow-y-auto px-4 py-3 space-y-1">
                  
                {messages().map(msg => <ChatMessage msg={msg} />)}
                <div ref={msgEndRef} />
              </div>

              {/* Streaming Content */}
              <Show when={agent.isStreaming && (streamingThink() || streamingText())}>
                <div class="mb-4 pl-2">
                  <Show when={streamingThink()}>
                    <ReasoningBlock content={streamingThink()} />
                  </Show>
                  <Show when={streamingText()}>
                    <div class="max-w-[85%] bg-[var(--bg-secondary)] border border-[var(--border)] rounded-2xl rounded-bl-md px-4 py-3 text-sm leading-relaxed shadow-sm">
                      <div class="whitespace-pre-wrap text-[var(--text)]">{streamingText()}</div>
                    </div>
                  </Show>
                </div>
              </Show>

              {/* Stream Indicator */}
              <StreamIndicator
                mode={streamKind() || 'idle'}
                toolNames={streamingToolNames()}
                secs={thinkingSecs()}
              />

              {/* Input Area */}
              <div class="border-t border-[var(--border)] p-3 shrink-0 bg-[var(--bg-primary)] transition-theme">
                <div class="flex items-end gap-2">
                  <textarea
                    ref={inputRef}
                    class="flex-1 bg-[var(--bg-secondary)] border border-[var(--border)] rounded-xl px-4 py-2.5 text-sm
                      text-[var(--text-h)] outline-none resize-none transition-colors
                      placeholder:text-[var(--muted)]
                      focus:border-[var(--accent)] focus:ring-1 focus:ring-[var(--accent)]/20"
                    rows={1}
                    placeholder={tt('chat.placeholder')}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send() }
                    }}
                    disabled={agent.isStreaming || !agent.state.connected}
                  />
                  <button
                    onClick={send}
                    disabled={agent.isStreaming || !agent.state.connected}
                    class="shrink-0 w-10 h-10 rounded-xl bg-[var(--accent)] text-white flex items-center justify-center
                      hover:brightness-110 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
                    aria-label={tt('chat.send')}
                  >
                    ↑
                  </button>
                </div>
              </div>
            </div>

            {/* Right Panel */}
            <div class={`shrink-0 border-l border-[var(--border)] transition-all duration-200 overflow-hidden ${rightOpen() ? 'w-56' : 'w-0'}`}>
              <div class="w-56 h-full overflow-y-auto p-3">
                <WorkspacePanel documents={docs.documents} recentEdits={docs.recentEdits} tasks={docs.tasks} />
              </div>
            </div>
          </div>

          {/* Modals */}
          <Show when={showSettings()}>
            <SettingsDialog onClose={() => { setShowSettings(false); setConfigVersion(v => v + 1) }} />
          </Show>
          <Show when={askUser()}>
            <AskUserDialog
              question={askUser()!.question}
              options={askUser()!.options}
              answer={askAnswer()}
              setAnswer={setAskAnswer}
              onSubmit={submitAskAnswer}
            />
          </Show>
        </div>
      </Show>
    </Show>
  )
}
