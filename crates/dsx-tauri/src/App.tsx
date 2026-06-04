// ── App Shell ──
// State management delegated to hooks. View composition only.

import { useState, useRef, useEffect, useCallback, type KeyboardEvent as ReactKeyboard } from 'react'
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
import { MarkdownBody } from './components/chat/MarkdownBody'
import { useToast } from './components/shared'
import { tt } from './i18n'

export default function App() {
  // ── Hooks ──
  const { config, loading: cfgLoading, checkDone } = useConfig()
  const agent = useAgent()
  const session = useSession()
  const balance = useBalance()
  const { setBalance } = balance
  const docs = useDocuments()
  const { addToast } = useToast()

  // ── UI state (view-only) ──
  const [messages, setMessages] = useState<Message[]>([])
  // input is now uncontrolled via inputRef
  const [thinkingSecs, setThinkingSecs] = useState(0)
  const [tokenUsage, setTokenUsage] = useState({ used: 0, limit: 150000 })
  const [cacheInfo, setCacheInfo] = useState({ hit: 0, miss: 0 })
  const [showSettings, setShowSettings] = useState(false)
  const [leftOpen, setLeftOpen] = useState(true)
  const [rightOpen, setRightOpen] = useState(true)
  const [askUser, setAskUser] = useState<{ question: string; options?: string[] } | null>(null)
  const [askAnswer, setAskAnswer] = useState('')
  const [auditLog, setAuditLog] = useState<Array<{ name: string; args: string; success: boolean }>>([])
  const [dsmlCount, setDsmlCount] = useState(0)
  const [, setConfigVersion] = useState(0)

  const inputRef = useRef<HTMLTextAreaElement>(null)
  const msgEndRef = useRef<HTMLDivElement>(null)
  const thinkStartRef = useRef(0)
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null)
  const loadMessagesRef = useRef(session.loadMessages)
  loadMessagesRef.current = session.loadMessages

  // ── Derived from useAgent (single source of truth) ──
  const connected = agent.state.connected
  const isStreaming = agent.isStreaming
  const streamMode: 'idle' | 'thinking' | 'tool_calling' | 'answering' = agent.state.stream.kind || 'idle'
  const streamToolNames = agent.state.stream.toolNames

  // ── Helper: push message ──
  const pushMsg = useCallback((msg: Message) => {
    setMessages(prev => [...prev, msg])
  }, [])

  // ── Helper: token counting ──
  const addTokens = useCallback((text: string) => {
    const count = Math.ceil(text.length / 3.5)
    setTokenUsage(prev => ({
      ...prev,
      used: Math.min(prev.used + count, prev.limit),
    }))
  }, [])

  // ── Rerender throttle (RAF) ──
  const rafPendingRef = useRef(false)
  const rerender = useCallback(() => {
    if (rafPendingRef.current) return
    rafPendingRef.current = true
    requestAnimationFrame(() => {
      rafPendingRef.current = false
      setTokenUsage(p => ({ ...p })) // trigger re-render
    })
  }, [])

  // ── Sync context limit from loaded config ──
  useEffect(() => {
    if (config?.context_limit) {
      setTokenUsage(prev => prev.limit === config.context_limit ? prev : { ...prev, limit: config.context_limit! })
    }
  }, [config])

  // ── Auto-start agent on launch ──
  useEffect(() => {
    if (checkDone && config && agent.statusChecked && !agent.state.connected && agent.state.status === 'idle') {
      agent.start()
    }
  }, [checkDone, config, agent.statusChecked, agent.state.connected, agent.state.status, agent.start])

  // ── Thinking timer sync with agent stream kind ──
  useEffect(() => {
    if (agent.state.stream.kind !== 'thinking') return
    thinkStartRef.current = Date.now()
    timerRef.current = setInterval(() => {
      setThinkingSecs(Math.floor((Date.now() - thinkStartRef.current) / 1000))
    }, 200)
    return () => {
      if (timerRef.current) { clearInterval(timerRef.current); timerRef.current = null }
      setThinkingSecs(0)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [agent.state.stream.kind])

  // ── Refresh session list on connect ──
  useEffect(() => {
    if (connected) session.refresh()
  }, [connected])

  // ── Auto-scroll chat to bottom ──
  useEffect(() => {
    msgEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages, streamMode])

  // ── Event handlers (non-streaming events — streaming handled by useAgent FSM) ──
  useEffect(() => {
    const unlist = listen<Record<string, unknown>>('agent-event', (e) => {
      const p = e.payload
      if (!p || typeof p.type !== 'string') return

      switch (p.type) {
        case 'assistant_msg': {
          const thinking = (p.thinking || '') as string
          const text = (p.text || '') as string
          const content = thinking ? `\n<reasoning>${thinking}</reasoning>\n${text}` : text
          pushMsg({ role: 'assistant', content })
          addTokens(content)
          rerender()
          break
        }
        case 'user_msg': {
          const text = (p.text as string) || ''
          if (text) { pushMsg({ role: 'user', content: text }); addTokens(text) }
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
          rerender()
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
          rerender()
          break
        }
        case 'turn_end': {
          const u = (p as any).usage
          if (u) {
            setTokenUsage(prev => ({
              used: u.prompt_tokens || prev.used,
              limit: (p as any).context_limit || prev.limit,
            }))
            setCacheInfo({ hit: u.prompt_cache_hit_tokens || 0, miss: u.prompt_cache_miss_tokens || 0 })
          }
          rerender()
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
          rerender()
          break
        }
        case 'balance': {
          const tb = (p as any).total_balance as string
          const cur = (p as any).currency as string
          if (tb && cur) {
            setBalance(`${tb} ${cur}`)
          }
          break
        }
        case 'session_restored': {
          setTokenUsage(prev => ({ ...prev, used: (p as any).tokens_used || prev.used }))
          // Load historical messages into chat
          const seed = (p as any).seed as string
          if (seed) {
            setMessages([])
            loadMessagesRef.current(seed).then(msgs => {
              setMessages((msgs as Message[]) || [])
            }).catch(() => { setMessages([]) })
          }
          break
        }
        case 'debug_snapshot': {
          docs.updateFromSnapshot({
            documents: (p as any).documents,
            recent_edits: (p as any).recent_edits,
            tasks: (p as any).tasks,
            })
            // Update cache info from snapshot (real-time during tool execution)
            const hit = (p as any).prompt_cache_hit_tokens
            const miss = (p as any).prompt_cache_miss_tokens
            if (typeof hit === 'number' && typeof miss === 'number') {
              setCacheInfo({ hit, miss })
            }
            // Update token usage from snapshot (real-time during tool execution)
            const ctx = (p as any).context_tokens
            if (typeof ctx === 'number' && ctx > 0) {
              setTokenUsage(prev => ({ ...prev, used: ctx }))
            }
          if (typeof (p as any).dsml_compat_count === 'number') {
            setDsmlCount((p as any).dsml_compat_count as number)
          }
          break
        }
        case 'error': {
          const msg = (p as any).message as string || 'Agent error'
          addToast(msg, 'error')
          pushMsg({ role: 'assistant', content: `\u26a0 ${msg}` })
          rerender()
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
    return () => { unlist.then(fn => fn()).catch(() => { setMessages([]) }) }
  }, [config, pushMsg, addTokens, rerender, balance, docs, addToast])

  // ── Auto-focus ──
  useEffect(() => { if (connected) inputRef.current?.focus() }, [connected])

  // ── Send message ──
  const send = useCallback(() => {
    const text = inputRef.current?.value?.trim()
    if (!text || isStreaming || !connected) return
    inputRef.current.value = ''
    pushMsg({ role: 'user', content: text })
    addTokens(text)
    agent.send(text)
    setTimeout(() => inputRef.current?.focus(), 50)
  }, [isStreaming, connected, pushMsg, addTokens, agent])
  // ── Ask answer submit ──
  const submitAskAnswer = useCallback(() => {
    if (!askUser) return
    const response = askAnswer.trim() || 'skipped'
    agent.send(response)
    setAskUser(null)
    setAskAnswer('')
  }, [askUser, askAnswer, agent])

  // ── Loading ──
  if (cfgLoading) return (
    <div className="h-screen flex items-center justify-center bg-[var(--bg-primary)]">
      <div className="flex flex-col items-center gap-4">
        <div className="w-10 h-10 rounded-full bg-[var(--accent)]/15 flex items-center justify-center anim-pulse-glow">
          <svg width="22" height="22" viewBox="0 0 32 32" fill="none" className="text-[var(--accent)]">
            <path d="M6 4h8l6 10-6 14H6l6-14L6 4z" fill="currentColor" opacity="0.8"/>
            <path d="M18 4h8l-6 14 6 14h-8l-6-14 6-14z" fill="currentColor"/>
          </svg>
        </div>
        <div className="text-sm text-[var(--muted)]">{tt('common.loading')}</div>
      </div>
    </div>
  )

  // ── Config wizard ──
  if (!checkDone) return <ConfigWizard onDone={() => { setConfigVersion(v => v + 1); window.location.reload() }} />

  // ── Keyboard handler ──
  const handleKey = (e: ReactKeyboard<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send() }
  }

    return (
      <div className="h-screen flex flex-col bg-[var(--bg-primary)] text-[var(--text)] overflow-hidden">
        {/* Top Bar */}
        <div className="h-10 flex items-center justify-between px-3 border-b border-[var(--border)] bg-[var(--bg-secondary)] shrink-0 transition-theme">
          <div className="flex items-center gap-2">
            <button onClick={() => setLeftOpen(o => !o)} className="w-7 h-7 rounded-md flex items-center justify-center hover:bg-[var(--bg-tertiary)] transition-colors" aria-label="Toggle left panel">
              <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor"><rect x="2" y="3" width="12" height="1.5" rx="0.5"/><rect x="2" y="7" width="12" height="1.5" rx="0.5"/><rect x="2" y="11" width="12" height="1.5" rx="0.5"/></svg>
            </button>
            <span className={`inline-block w-2 h-2 rounded-full ${connected ? 'bg-[var(--success)]' : 'bg-[var(--muted)]'}`} />
            <span className="text-xs text-[var(--text)] font-medium">{connected ? 'Agent 在线' : '未连接'}</span>
          </div>
          <div className="flex items-center gap-3">
            <div className="text-xs text-[var(--muted)]">上下文 {tokenUsage.used} / {tokenUsage.limit}</div>
            {dsmlCount > 0 && <div className="text-xs text-[var(--accent)] font-medium">DSML ×{dsmlCount}</div>}
            {isStreaming && (
              <button onClick={() => agent.cancel()} className="text-xs text-[var(--error)] hover:underline">停止</button>
            )}
            <button onClick={() => setRightOpen(o => !o)} className="w-7 h-7 rounded-md flex items-center justify-center hover:bg-[var(--bg-tertiary)] transition-colors" aria-label="Toggle right panel">
              <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor"><rect x="2" y="3" width="12" height="1.5" rx="0.5"/><rect x="2" y="7" width="12" height="1.5" rx="0.5"/><rect x="2" y="11" width="12" height="1.5" rx="0.5"/></svg>
            </button>
          </div>
        </div>

        {/* Main Content */}
        <div className="flex-1 flex overflow-hidden">
          {/* Left Panel */}
          <div className={`shrink-0 border-r border-[var(--border)] transition-all duration-200 overflow-hidden ${leftOpen ? 'w-56' : 'w-0'}`}>
            <div className="w-56 h-full overflow-y-auto p-3 space-y-3">
              <InfoPanel
                tokens={tokenUsage}
                cache={cacheInfo}
                balance={balance.balance}
                sessionId={agent.state.sessionId || ''}
                  sessions={session.sessions}
                auditLog={auditLog}
                toolBatch={null}
                toolNames={streamToolNames}
                onSettings={() => setShowSettings(true)}
                onNewSession={agent.start}
                onResumeSession={seed => agent.resume(seed)}
                onDeleteAllSessions={session.deleteAll}
                onDeleteSession={seed => session.deleteSession(seed)}
                onRefreshBalance={() => config?.api_key && balance.refresh(config.api_key)}
              />
            </div>
          </div>

          {/* Chat Area */}
          <div className="flex-1 flex flex-col min-w-0">
            <div className="flex-1 overflow-y-auto px-4 py-4 space-y-4">
              {messages.map((msg, i) => (
                <ChatMessage key={i} msg={msg} />
              ))}
              {isStreaming && (agent.streamReasoning || agent.streamContent) && (
                <div className="mb-4 anim-msg-in">
                  {agent.streamReasoning && (
                    <ReasoningBlock content={agent.streamReasoning} />
                  )}
                  {agent.streamContent && (
                    <div className="max-w-[85%] bg-[var(--bg-secondary)] border border-[var(--border)] rounded-2xl rounded-bl-md px-4 py-3 text-sm leading-relaxed shadow-sm">
                      <MarkdownBody content={agent.streamContent} />
                    </div>
                  )}
                </div>
              )}
              <StreamIndicator mode={streamMode} toolNames={streamToolNames} secs={thinkingSecs} />
              <div ref={msgEndRef} />
            </div>

            {/* Input Area */}
            <div className="border-t border-[var(--border)] bg-[var(--bg-secondary)] px-4 py-3 shrink-0 transition-theme">
              <div className="flex items-end gap-2 max-w-3xl mx-auto">
                <textarea
                  ref={inputRef}
                  defaultValue=""
                  rows={1}
                  placeholder="输入消息... (Enter 发送, Shift+Enter 换行)"
                  disabled={!connected || isStreaming}
                  className="flex-1 resize-none bg-[var(--bg-primary)] border border-[var(--border)] rounded-xl px-3.5 py-2.5 text-sm text-[var(--text-h)] font-mono outline-none transition-colors placeholder:text-[var(--muted)] focus:border-[var(--accent)] focus:ring-1 focus:ring-[var(--accent)]/20 disabled:opacity-50"
                  aria-label="输入消息"
                />
                <button
                  onClick={send}
                  disabled={!connected || isStreaming}
                  className="shrink-0 w-9 h-9 rounded-xl bg-[var(--accent)] text-white flex items-center justify-center hover:brightness-110 disabled:opacity-40 disabled:cursor-not-allowed transition-all"
                  aria-label="发送"
                >
                  <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor"><path d="M1.5 2l13 6-13 6 3-6-3-6z"/></svg>
                </button>
              </div>
            </div>
          </div>

          {/* Right Panel */}
          <div className={`shrink-0 border-l border-[var(--border)] transition-all duration-200 overflow-hidden ${rightOpen ? 'w-56' : 'w-0'}`}>
            <div className="w-56 h-full overflow-y-auto">
              <WorkspacePanel documents={docs.documents} recentEdits={docs.recentEdits} tasks={docs.tasks} />
            </div>
          </div>
        </div>

        {/* Modals */}
        {showSettings && <SettingsDialog onClose={() => { setShowSettings(false); setConfigVersion(v => v + 1) }} />}
        {askUser && <AskUserDialog question={askUser.question} options={askUser.options}
          answer={askAnswer} setAnswer={setAskAnswer} onSubmit={submitAskAnswer} />}
        </div>
    )
}