// ── App Shell (SolidJS) ──
// State management delegated to hooks. View composition only.
// Migrated from React — all event handlers match original exactly.

import { createSignal, createEffect, onMount, onCleanup, Show, For } from 'solid-js'
import { listen } from '@tauri-apps/api/event'
import { useAgent, useConfig, useSession, useBalance, useDocuments } from './hooks'
import { api } from './bridge/tauri'
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

function loadPanelWidth(key: string, fallback: number): number {
  try { const v = localStorage.getItem(key); if (v) return Math.max(120, parseInt(v, 10)) } catch {}
  return fallback
}

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
  const [leftWidth, setLeftWidth] = createSignal(loadPanelWidth('dsx:leftPanelWidth', 224))
  const [rightWidth, setRightWidth] = createSignal(loadPanelWidth('dsx:rightPanelWidth', 260))
  const [dragging, setDragging] = createSignal<null | 'left' | 'right'>(null)

  let resizeStartX = 0
  let resizeStartWidth = 0
  const [askUser, setAskUser] = createSignal<{ question: string; options?: string[] } | null>(null)
  const [askAnswer, setAskAnswer] = createSignal('')
  const [auditLog, setAuditLog] = createSignal<Array<{ name: string; args: string; success: boolean }>>([])
  const [streamingThink, setStreamingThink] = createSignal('')
  const [streamingText, setStreamingText] = createSignal('')
  const [streamingToolNames, setStreamingToolNames] = createSignal<string[]>([])
  const [streamKind, setStreamKind] = createSignal<'thinking' | 'tool_calling' | 'answering' | null>(null)

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
    if (cfg.checkDone && cfg.config && agent.statusChecked && !agent.state.connected && (agent.state.status === 'idle' || agent.state.status === 'connecting')) {
      agent.start()
    }
  })

  // ── Auto-create or resume session after agent start ──
  createEffect(() => {
    if (agent.state.connected && !agent.state.sessionId) {
      const sessions = agent.state.sessions
      if (sessions.length > 0) {
        agent.resume(sessions[0].seed)
      } else {
        agent.createSession()
      }
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

  // ── Auto-scroll chat to bottom (only near bottom) ──
  createEffect(() => {
    messages()
    agent.state.streaming
    const el = (msgEndRef as HTMLDivElement)?.parentElement
    if (el) {
      const dist = el.scrollHeight - el.scrollTop - el.clientHeight
      if (dist < 120) msgEndRef?.scrollIntoView({ behavior: 'auto' })
    }
  })

  // ── Auto-focus input on connect ──
  createEffect(() => {
    if (agent.state.connected) inputRef?.focus()
  })

  // ── Panel resize drag ──
  createEffect(() => {
    const side = dragging()
    if (!side) return
    const isLeft = side === 'left'
    const onMove = (e: MouseEvent) => {
      e.preventDefault()
      const delta = isLeft ? e.clientX - resizeStartX : resizeStartX - e.clientX
      const w = Math.max(isLeft ? 160 : 200, Math.min(isLeft ? 400 : 480, resizeStartWidth + delta))
      if (isLeft) setLeftWidth(w); else setRightWidth(w)
    }
    const onUp = () => {
      setDragging(null)
      const w = isLeft ? leftWidth() : rightWidth()
      try { localStorage.setItem(isLeft ? 'dsx:leftPanelWidth' : 'dsx:rightPanelWidth', String(w)) } catch {}
    }
    document.addEventListener('mousemove', onMove)
    document.addEventListener('mouseup', onUp, { once: true })
    document.body.style.cursor = 'col-resize'
    document.body.style.userSelect = 'none'
    onCleanup(() => {
      document.removeEventListener('mousemove', onMove)
      document.removeEventListener('mouseup', onUp)
      document.body.style.cursor = ''
      document.body.style.userSelect = ''
    })
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
        case 'turn_start': {
          agent.dispatch({ type: 'TURN_START', turn_id: p.turn_id as string, user_text: p.user_text as string })
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
        case 'done': {
          break
        }
        case 'cancelled': {
          setAskUser(null)
          setAskAnswer('')
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
    inputRef?.focus()
  }

  // ── Ask answer submit ──
  const submitAskAnswer = () => {
    if (!askUser()) return
    const response = askAnswer().trim()
    if (!response) return
    agent.send(response)
    setAskUser(null)
    setAskAnswer('')
  }

  const dismissAskUser = () => {
    agent.send('[SKIPPED]')
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
        <ConfigWizard onDone={() => { window.location.reload() }} />
      </Show>

      {/* Main App */}
      <Show when={cfg.checkDone}>
        <div class="h-screen flex flex-col bg-[var(--bg-primary)] text-[var(--text)] overflow-hidden relative">
          {/* Top Bar */}
          <div class="h-10 flex items-center justify-between px-3 border-b border-[var(--border)] bg-[var(--bg-secondary)] shrink-0 transition-theme">
            <div class="flex items-center gap-2 min-w-0">
              <button onClick={() => setLeftOpen(o => !o)} class="w-7 h-7 rounded-md flex items-center justify-center hover:bg-[var(--bg-tertiary)] transition-colors shrink-0" aria-label="Toggle left panel">
                <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor"><rect x="2" y="3" width="12" height="1.5" rx="0.5"/><rect x="2" y="7" width="12" height="1.5" rx="0.5"/><rect x="2" y="11" width="12" height="1.5" rx="0.5"/></svg>
              </button>
              <Show when={cfg.config?.model}>
                <span class="text-[11px] font-mono text-[var(--accent)] bg-[var(--accent)]/8 px-2 py-0.5 rounded-md shrink-0">{cfg.config!.model}</span>
              </Show>
              <span class="text-xs text-[var(--muted)] truncate">{agent.state.sessionId || ''}</span>
            </div>
            <div class="flex items-center gap-2 shrink-0">
              <button onClick={() => setShowSettings(true)} class="w-7 h-7 rounded-md flex items-center justify-center hover:bg-[var(--bg-tertiary)] transition-colors" aria-label="Settings" title={tt('settings.title')}>
                <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor"><path d="M8 10a2 2 0 100-4 2 2 0 000 4zM14.3 8.5l-1.3-.3a5 5 0 00-.4-1l.7-1.2a.3.3 0 00-.1-.4l-1.1-1.1a.3.3 0 00-.4-.1l-1.2.7a5 5 0 00-1-.4L9 3.5a.3.3 0 00-.3-.3H6.3a.3.3 0 00-.3.3l-.3 1.3a5 5 0 00-1 .4L3.5 4.5a.3.3 0 00-.4.1L2 5.7a.3.3 0 00-.1.4l.7 1.2a5 5 0 00-.4 1L1 8.7a.3.3 0 00.3.3h1.5l.3 1.3a5 5 0 001 .4l-.7 1.2a.3.3 0 00.1.4l1.1 1.1a.3.3 0 00.4.1l1.2-.7a5 5 0 001 .4l.3 1.3a.3.3 0 00.3.3h1.5a.3.3 0 00.3-.3l.3-1.3a5 5 0 001-.4l1.2.7a.3.3 0 00.4-.1l1.1-1.1a.3.3 0 00.1-.4l-.7-1.2a5 5 0 00.4-1l1.3-.3a.3.3 0 00.3-.3V6.5a.3.3 0 00-.3-.3z"/></svg>
              </button>
              <button onClick={() => setRightOpen(o => !o)} class="w-7 h-7 rounded-md flex items-center justify-center hover:bg-[var(--bg-tertiary)] transition-colors" aria-label="Toggle right panel">
                <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor"><rect x="2" y="3" width="12" height="1.5" rx="0.5"/><rect x="2" y="7" width="12" height="1.5" rx="0.5"/><rect x="2" y="11" width="12" height="1.5" rx="0.5"/></svg>
              </button>
            </div>
          </div>

          {/* Main Content */}
          <div class="flex-1 flex overflow-hidden">
            {/* Left Panel */}
            <div class={`shrink-0 border-r border-[var(--border)] overflow-hidden ${!dragging() ? 'transition-[width] duration-200' : ''}`} style={{ width: leftOpen() ? `${leftWidth()}px` : '0px' }}>
              <div class="h-full overflow-y-auto p-3 space-y-3" style={{ width: `${leftWidth()}px` }}>
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

            {/* Left resize handle */}
            <Show when={leftOpen()}>
              <div
                class="w-1 shrink-0 cursor-col-resize hover:bg-[var(--accent)]/40 active:bg-[var(--accent)]/60 transition-colors"
                onMouseDown={(e) => { resizeStartX = e.clientX; resizeStartWidth = leftWidth(); setDragging('left') }}
              />
            </Show>

            {/* Center Chat */}
            <div class="flex-1 flex flex-col min-w-0">
              {/* Messages */}
              <div class="flex-1 overflow-y-auto px-4 py-3 space-y-1">
                  
                <For each={messages()}>{msg => <ChatMessage msg={msg} />}</For>

                <Show when={agent.isStreaming && (streamingThink() || streamingText() || (streamKind() === 'tool_calling' && streamingToolNames().length > 0))}>
                  <div class="mb-4 pl-2">
                    <Show when={streamingThink()}>
                      <ReasoningBlock content={streamingThink()} />
                    </Show>
                    <Show when={streamKind() === 'tool_calling' && streamingToolNames().length > 0}>
                      <ToolCallPreview names={streamingToolNames()} />
                    </Show>
                    <Show when={streamingText()}>
                      <div class="max-w-[85%] bg-[var(--bg-secondary)] border border-[var(--border)] rounded-2xl rounded-bl-md px-4 py-3 text-[15px] leading-relaxed shadow-sm">
                        <div class="whitespace-pre-wrap text-[var(--text)]">{streamingText()}</div>
                      </div>
                    </Show>
                  </div>
                </Show>

                <div ref={msgEndRef} />
              </div>

              {/* Stream Indicator */}
              <StreamIndicator
                mode={streamKind() || 'idle'}
                toolNames={streamingToolNames()}
                secs={thinkingSecs()}
              />

              {/* Input Area */}
              <div class="border-t border-[var(--border)] p-3 shrink-0 bg-[var(--bg-primary)] transition-theme">
                <div class="flex items-end gap-2">
                  <button
                    onClick={() => agent.state.connected && api.reloadAgent().catch(e => console.error('reload failed:', e))}
                    class="shrink-0 w-10 h-10 rounded-xl bg-[var(--bg-tertiary)] text-[var(--muted)] flex items-center justify-center
                      hover:bg-[var(--accent)]/15 hover:text-[var(--accent)] transition-colors"
                    title={tt('chat.reloadConfig')}
                    aria-label={tt('chat.reloadConfig')}
                  >
                    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5">
                      <path d="M2 8a6 6 0 0 1 10.47-4M14 8a6 6 0 0 1-10.47 4"/>
                      <path d="M12 2v2.5H9.5M4 14v-2.5h2.5"/>
                    </svg>
                  </button>
                  <textarea
                    ref={inputRef}
                    class="flex-1 bg-[var(--bg-secondary)] border border-[var(--border)] rounded-xl px-4 py-2.5 text-[15px]
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
                    onClick={() => agent.isStreaming ? agent.cancel() : send()}
                    disabled={!agent.state.connected}
                    class={`shrink-0 w-10 h-10 rounded-xl flex items-center justify-center transition-all duration-200
                      disabled:opacity-30 disabled:cursor-not-allowed
                      ${agent.isStreaming
                        ? 'bg-[var(--error)] hover:bg-[var(--error)]/80 text-white shadow-[0_0_12px_rgba(239,68,68,0.3)]'
                        : 'bg-[var(--accent)] hover:brightness-110 text-white'}`}
                    aria-label={agent.isStreaming ? tt('chat.stop') : tt('chat.send')}
                  >
                    {agent.isStreaming ? (
                      <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor"><rect x="3" y="3" width="10" height="10" rx="2"/></svg>
                    ) : (
                      <span class="text-lg leading-none">↑</span>
                    )}
                  </button>
                </div>
              </div>
            </div>

            {/* Right resize handle */}
            <Show when={rightOpen()}>
              <div
                class="w-1 shrink-0 cursor-col-resize hover:bg-[var(--accent)]/40 active:bg-[var(--accent)]/60 transition-colors"
                onMouseDown={(e) => { resizeStartX = e.clientX; resizeStartWidth = rightWidth(); setDragging('right') }}
              />
            </Show>

            {/* Right Panel */}
            <div class={`shrink-0 border-l border-[var(--border)] overflow-hidden ${!dragging() ? 'transition-[width] duration-200' : ''}`} style={{ width: rightOpen() ? `${rightWidth()}px` : '0px' }}>
              <div class="h-full overflow-y-auto p-3" style={{ width: `${rightWidth()}px` }}>
                <WorkspacePanel documents={docs.documents} recentEdits={docs.recentEdits} tasks={docs.tasks} />
              </div>
            </div>
          </div>

          {/* Modals */}
          <Show when={showSettings()}>
            <SettingsDialog onClose={() => { setShowSettings(false) }} />
          </Show>
          <Show when={askUser()}>
            <AskUserDialog
              question={askUser()!.question}
              options={askUser()!.options}
              answer={askAnswer()}
              setAnswer={setAskAnswer}
              onSubmit={submitAskAnswer}
              onDismiss={dismissAskUser}
            />
          </Show>
        </div>
      </Show>
    </Show>
  )
}

// ── ToolCallPreview: shown in-stream when first tool call delta arrives ──

function ToolCallPreview(props: { names: string[] }) {
  const delays = ['0ms', '60ms', '120ms', '180ms', '240ms', '300ms']
  const visible = () => props.names.slice(0, 8)
  const more = () => props.names.length > 8 ? props.names.length - 8 : 0
  return (
    <div class="flex items-start gap-2.5 mb-1 anim-fade-in">
      <div class="w-7 h-7 rounded-full bg-[var(--accent)]/15 flex items-center justify-center shrink-0 mt-0.5 anim-pulse-glow">
        <svg width="14" height="14" viewBox="0 0 16 16" class="text-[var(--accent)]">
          <circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" stroke-width="1.5" stroke-dasharray="6 2">
            <animateTransform attributeName="transform" type="rotate" from="0 8 8" to="360 8 8" dur="1.2s" repeatCount="indefinite"/>
          </circle>
        </svg>
      </div>
      <div class="inline-block max-w-[85%] rounded-2xl px-4 py-2.5 text-sm bg-[var(--bg-secondary)] border border-[var(--accent)]/30 rounded-bl-md shadow-[0_0_12px_rgba(124,58,237,0.12)] transition-theme">
        <div class="text-[var(--accent)] font-medium mb-1.5">工具调用准备中... <Show when={more() > 0}><span class="text-[var(--muted)]">(+{more()})</span></Show></div>
        <div class="flex flex-wrap gap-1.5">
          <For each={visible()}>{(n, i) => (
            <span
              class="text-xs font-mono bg-[var(--accent)]/10 text-[var(--accent)] px-2 py-1 rounded-md border border-[var(--accent)]/20 animate-task-enter"
              style={{ 'animation-delay': delays[i() % delays.length] ?? '0ms' } as any}
            >
              {n}
            </span>
          )}</For>
        </div>
      </div>
    </div>
  )
}
