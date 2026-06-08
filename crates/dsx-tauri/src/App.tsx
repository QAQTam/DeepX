// ── App Shell (SolidJS) ──
// State management delegated to hooks. View composition only.
// Event handling extracted to useAgentEvents hook.

import { createSignal, createEffect, Show, For } from 'solid-js'
import { createVirtualizer } from '@tanstack/solid-virtual'
import { useAgent, useConfig, useSession, useBalance, useDocuments, useAgentEvents } from './hooks'
import { LiveOutputContext } from './hooks/LiveOutputContext'
import { api } from './bridge/tauri'
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
import type { Message } from './types'

// ── Heuristic: estimate message height for virtual list ──
// Only used for brand-new items before the first sync measureElement().
// After measurement, itemSizeCache holds the actual height.
function estimateMessageHeight(msg: Message): number {
  if (msg.role === 'user') {
    return Math.max(48, Math.min(500, 32 + msg.content.length * 0.45))
  }
  if (msg.role === 'system') {
    return Math.max(40, Math.min(300, 30 + msg.content.length * 0.4))
  }
  if (!msg.blocks) return 80
  let h = 60
  for (const b of msg.blocks) {
    if (b.type === 'reasoning') h += Math.max(80, Math.min(1200, b.content.length * 0.4))
    else if (b.type === 'text') h += Math.max(0, Math.min(1000, b.content.length * 0.45))
    else if (b.type === 'tool') h += 90
  }
  return Math.max(80, Math.min(2400, h))
}

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
  const events = useAgentEvents(agent, session, docs, balance, toast)

  // ── UI state ──
  const [showSettings, setShowSettings] = createSignal(false)
  const [leftOpen, setLeftOpen] = createSignal(true)
  const [rightOpen, setRightOpen] = createSignal(true)
  const [leftWidth, setLeftWidth] = createSignal(loadPanelWidth('dsx:leftPanelWidth', 224))
  const [rightWidth, setRightWidth] = createSignal(loadPanelWidth('dsx:rightPanelWidth', 260))
  const [dragging, setDragging] = createSignal<null | 'left' | 'right'>(null)

  let scrollContainerRef!: HTMLDivElement

  // ── Virtual list — directly tracks events.messages() ──
  // No separate windowed slice: virtualizer only renders visible items in DOM.
  // The store cap (MAX_MESSAGES=200) bounds memory; virtualizer bounds DOM nodes.
  const virtualizer = createVirtualizer({
    count: events.messages().length,
    getScrollElement: () => scrollContainerRef,
    estimateSize: (i: number) => estimateMessageHeight(events.messages()[i]),
    overscan: 5,
    paddingStart: 16,
    paddingEnd: 16,
  })

  // ── Keep virtualizer count + estimate reactive ──
  createEffect(() => {
    const msgs = events.messages()
    const v = virtualizer as any
    if (v.options) {
      v.options.count = msgs.length
      v.options.estimateSize = (i: number) => estimateMessageHeight(msgs[i])
      v.notify(false)
    }
  })

  // ── After content changes (tool_results, expand/collapse),
  //     ResizeObserver runs as a microtask and updates the cache.
  //     This effect fires a notify AFTER ResizeObserver to reconcile
  //     item positions with the updated measurements — all before paint.
  let _notifyMicro: number | null = null
  createEffect(() => {
    events.contentVersion()
    events.msgCount()
    if (_notifyMicro == null) {
      _notifyMicro = requestAnimationFrame(() => {
        _notifyMicro = null
        ;(virtualizer as any).notify(false)
      })
    }
  })

  // ── Panel drag resize ──
  let _dragStartX = 0
  let _dragStartW = 0
  let inputRef!: HTMLTextAreaElement
  let msgEndRef!: HTMLDivElement

  const onDragMove = (e: MouseEvent) => {
    const dir = dragging()
    if (!dir) return
    const dx = e.clientX - _dragStartX
    if (dir === 'left') setLeftWidth(Math.max(120, _dragStartW + dx))
    else setRightWidth(Math.max(120, _dragStartW - dx))
  }
  const onDragEnd = () => {
    const dir = dragging()
    if (!dir) return
    setDragging(null)
    try {
      if (dir === 'left') localStorage.setItem('dsx:leftPanelWidth', String(leftWidth()))
      else localStorage.setItem('dsx:rightPanelWidth', String(rightWidth()))
    } catch {}
  }

  // ── Sync context limit from loaded config ──
  createEffect(() => {
    const c = cfg.config
    if (c?.context_limit) {
      events.setTokenUsage(prev => prev.limit === c.context_limit ? prev : { ...prev, limit: c.context_limit! })
    }
  })

  // ── Auto-start agent on launch ──
  createEffect(() => {
    if (cfg.checkDone && cfg.config && agent.statusChecked && !agent.state.connected && (agent.state.status === 'idle' || agent.state.status === 'connecting')) {
      agent.start()
    }
  })

  // ── Auto-create or resume session after agent start ──
  let sessionHandled = false
  createEffect(() => {
    if (agent.state.connected && !agent.state.sessionId) {
      if (sessionHandled) return
      sessionHandled = true
      const sessions = agent.state.sessions
      if (sessions.length > 0) {
        agent.resume(sessions[0].seed)
      } else {
        agent.createSession()
      }
    }
    if (agent.state.sessionId) sessionHandled = false
  })

  // ── Refresh session list on connect ──
  let lastConnected = false
  createEffect(() => {
    const connected = agent.state.connected
    if (connected && !lastConnected) session.refresh()
    lastConnected = connected
  })

  // ── Auto-scroll chat to bottom (only when user is near bottom) ──
  let _autoScrollRaf = 0
  createEffect(() => {
    events.msgCount()
    events.contentVersion()
    agent.state.streaming
    const el = scrollContainerRef
    if (!el) return
    const dist = el.scrollHeight - el.scrollTop - el.clientHeight
    if (dist < 300) {
      if (_autoScrollRaf) cancelAnimationFrame(_autoScrollRaf)
      _autoScrollRaf = requestAnimationFrame(() => {
        _autoScrollRaf = 0
        msgEndRef?.scrollIntoView({ block: 'end' })
      })
    }
  })

  // ── Send message ──
  const send = () => {
    const text = inputRef?.value?.trim()
    if (!text || agent.isStreaming || !agent.state.connected) return
    inputRef.value = ''
    events.setMessages(prev => [...prev, { role: 'user', content: text }])
    agent.send(text)
    inputRef?.focus()
  }

  // ── Ask answer submit ──
  const submitAskAnswer = () => {
    if (!events.askUser()) return
    const response = events.askAnswer().trim()
    if (!response) return
    agent.send(response)
    events.setAskUser(null)
    events.setAskAnswer('')
  }

  const dismissAskUser = () => {
    agent.send('[SKIPPED]')
    events.setAskUser(null)
    events.setAskAnswer('')
  }

  // ── Render ──
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
        <div
          class="h-screen flex flex-col bg-[var(--bg-primary)] text-[var(--text)] overflow-hidden relative"
          onMouseMove={onDragMove}
          onMouseUp={onDragEnd}
        >
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
                  tokens={events.tokenUsage}
                  cache={events.cacheInfo}
                  balance={balance.balance}
                  sessionId={() => agent.state.sessionId || ''}
                  sessions={() => session.sessions}
                  auditLog={events.auditLog()}
                  toolBatch={null}
                  toolNames={events.streamingToolNames()}
                  onSettings={() => setShowSettings(true)}
                  onNewSession={() => agent.createSession()}
                  onResumeSession={seed => agent.resume(seed)}
                  onDeleteAllSessions={session.deleteAll}
                  onDeleteSession={seed => session.deleteSession(seed)}
                  onRefreshBalance={() => cfg.config?.api_key && balance.refresh(cfg.config.api_key).catch(e => toast.addToast('Balance refresh failed: ' + String(e), 'error'))}
                />
              </div>
            </div>

            {/* Left resize handle */}
            <Show when={leftOpen()}>
              <div
                class="w-1 shrink-0 cursor-col-resize hover:bg-[var(--accent)]/40 active:bg-[var(--accent)]/60 transition-colors"
                onMouseDown={(e) => { _dragStartX = e.clientX; _dragStartW = leftWidth(); setDragging('left') }}
              />
            </Show>

            {/* Center Chat */}
            <LiveOutputContext.Provider value={{ liveOutputs: events.liveToolOutputs, notifyResize: events.notifyResize }}>
            <div class="flex-1 flex flex-col min-w-0">
              {/* Messages */}
              <div ref={scrollContainerRef} class="flex-1 overflow-y-auto px-4 py-3">

                <div style={{ height: `${virtualizer.getTotalSize()}px`, width: "100%", position: "relative" }}>
                  <For each={virtualizer.getVirtualItems()}>
                    {(vItem) => {
                      const msg = events.messages()[vItem.index]
                      return (
                        <div
                          style={{ position: "absolute", top: `${vItem.start}px`, width: "100%" }}
                          data-index={vItem.index}
                          ref={(el) => { if (el && el.hasAttribute('data-index')) virtualizer.measureElement(el) }}
                        >
                          <ChatMessage msg={msg} />
                        </div>
                      )
                    }}
                  </For>
                </div>

                <Show when={agent.isStreaming && (events.streamingThink() || events.streamingText() || (events.streamKind() === 'tool_calling' && events.streamingToolNames().length > 0))}>
                  <div class="mb-4 pl-2">
                    <Show when={events.streamingThink()}>
                      <ReasoningBlock content={events.streamingThink()} />
                    </Show>
                    {/* Answer BEFORE tool calls — matches LLM output order */}
                    <Show when={events.streamingText()}>
                      <div class="max-w-[85%] bg-[var(--bg-secondary)] border border-[var(--border)] rounded-2xl rounded-bl-md px-4 py-3 text-[15px] leading-relaxed shadow-sm">
                        <div class="whitespace-pre-wrap text-[var(--text)]">{events.streamingText()}</div>
                      </div>
                    </Show>
                    <Show when={events.streamKind() === 'tool_calling' && events.streamingToolNames().length > 0}>
                      <ToolCallPreview names={events.streamingToolNames()} />
                    </Show>
                  </div>
                </Show>

                <div ref={msgEndRef} />
              </div>

              {/* Stream Indicator */}
              <StreamIndicator
                mode={events.streamKind() || 'idle'}
                toolNames={events.streamingToolNames()}
                secs={events.thinkingSecs()}
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
            </LiveOutputContext.Provider>

            {/* Right resize handle */}
            <Show when={rightOpen()}>
              <div
                class="w-1 shrink-0 cursor-col-resize hover:bg-[var(--accent)]/40 active:bg-[var(--accent)]/60 transition-colors"
                onMouseDown={(e) => { _dragStartX = e.clientX; _dragStartW = rightWidth(); setDragging('right') }}
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
          <Show when={events.askUser()}>
            <AskUserDialog
              question={events.askUser()!.question}
              options={events.askUser()!.options}
              answer={events.askAnswer()}
              setAnswer={v => events.setAskAnswer(v)}
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
        <div class="text-[var(--accent)] font-medium mb-1.5">{tt('chat.toolPreparing')} <Show when={more() > 0}><span class="text-[var(--muted)]">(+{more()})</span></Show></div>
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
