import { useState, useRef, useEffect, useCallback, type KeyboardEvent } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { T } from './i18n'
import type { Message, DocInfo } from './types'
import { ChatMessage } from './components/ChatMessage'
import { InfoPanel } from './components/InfoPanel'
import { WorkspacePanel } from './components/WorkspacePanel'
import { StreamIndicator } from './components/StreamIndicator'
import { ConfigWizard } from './components/ConfigWizard'
import { SettingsDialog } from './components/SettingsDialog'
import { AskUserDialog } from './components/AskUserDialog'

export default function App() {
  const [configDone, setConfigDone] = useState(false)
  const [checking, setChecking] = useState(true)
  const [connected, setConnected] = useState(false)
  const [messages, setMessages] = useState<Message[]>([])
  const [input, setInput] = useState('')
  const [isStreaming, setIsStreaming] = useState(false)
  const [streamMode, setStreamMode] = useState<'idle' | 'thinking' | 'tool_calling' | 'answering'>('idle')
  const [toolNames, setToolNames] = useState<string[]>([])
  const [thinkingSecs, setThinkingSecs] = useState(0)
  const thinkStartRef = useRef(0)
  const timerRef = useRef<number | undefined>(undefined)
  const streamModeRef = useRef<'idle' | 'thinking' | 'tool_calling' | 'answering'>('idle')
  const setStream = (mode: 'idle' | 'thinking' | 'tool_calling' | 'answering') => {
    streamModeRef.current = mode
    setStreamMode(mode)
    if (mode === 'thinking') {
      thinkStartRef.current = Date.now(); setThinkingSecs(0)
      timerRef.current = window.setInterval(() => setThinkingSecs(Math.floor((Date.now() - thinkStartRef.current) / 1000)), 200)
    } else {
      if (timerRef.current !== undefined) { clearInterval(timerRef.current); timerRef.current = undefined }
    }
    if (mode === 'idle') setToolNames([])
  }
  const [sessionId, setSessionId] = useState('')
  const [tokenUsage, setTokenUsage] = useState<{ used: number; limit: number }>({ used: 0, limit: 150000 })
  const [cacheInfo, setCacheInfo] = useState<{ hit: number; miss: number }>({ hit: 0, miss: 0 })
  const [balance, setBalance] = useState('')
  const fetchBalance = useCallback(() => {
    invoke<any>('load_config').then((c: any) => {
      if (c.api_key) invoke<any>('get_balance', { apiKey: c.api_key }).then((r: any) => {
        if (r?.balance_infos?.[0]) setBalance(`${r.balance_infos[0].total_balance} ${r.balance_infos[0].currency}`)
      }).catch(() => {})
    }).catch(() => {})
  }, [])
  useEffect(() => { if (connected) fetchBalance() }, [connected, fetchBalance])
  const [showSettings, setShowSettings] = useState(false)
  const [configInfo, setConfigInfo] = useState({ model: '', effort: '' })
  const [modelOptions, setModelOptions] = useState<string[]>([])
  const [askUser, setAskUser] = useState<{ question: string; options?: string[] } | null>(null)
  const [askAnswer, setAskAnswer] = useState('')
  const [sessions, setSessions] = useState<any[]>([])
  const refreshSessions = useCallback(() => {
    invoke<any[]>('cmd_sessions').then(setSessions).catch(() => {})
  }, [])
  const [documents, setDocuments] = useState<DocInfo[]>([])
  const [recentEdits, setRecentEdits] = useState<string[]>([])
  const [taskList, setTaskList] = useState<any[]>([])
  const [dsmlCompat, setDsmlCompat] = useState(0)
  const [configVersion, setConfigVersion] = useState(0)
  const [leftOpen, setLeftOpen] = useState(true)
  const [rightOpen, setRightOpen] = useState(true)

  // v4.1: streaming state simplified to single content + reasoning refs
  const streamRef = useRef({ content: '', reasoning: '', toolCards: [] as any[] })
  const [tick, setTick] = useState(0); const chatEnd = useRef<HTMLDivElement>(null); const inputRef = useRef<HTMLTextAreaElement>(null)
  const connectingRef = useRef(false); const restartingRef = useRef(false)
  const rafPending = useRef(false)
  const rafId = useRef(0)
  const msgSeq = useRef(0)
  const pushMsg = (msg: Message) => { msgSeq.current++; (msg as any)._id = msgSeq.current; setMessages(p => [...p, msg]) }
  const rerender = useCallback(() => {
    if (rafPending.current) return
    rafPending.current = true
    rafId.current = requestAnimationFrame(() => {
      rafPending.current = false
      setTick(n => n + 1)
    })
  }, [])
  useEffect(() => () => { cancelAnimationFrame(rafId.current) }, [])

  useEffect(() => {
    invoke<any>('load_config').then((c: any) => {
      const limit = c.context_limit || 150000
      setTokenUsage(p => ({ ...p, limit }))
      setConfigInfo({ model: c.model || '', effort: c.effort || '' })
      let cached = c.cached_models
      if (typeof cached === 'string') try { cached = JSON.parse(cached) } catch { /* ignore */ }
      if (Array.isArray(cached)) setModelOptions(cached)
    }).catch(() => {})
  }, [configVersion])

  const handleDeleteAllSessions = () => {
    invoke('delete_all_sessions').then(() => {
      setMessages([]); setSessionId('')
      setDocuments([]); setRecentEdits([])
      setCacheInfo({ hit: 0, miss: 0 })
      setTokenUsage(p => ({ ...p, used: 0 }))
      invoke('start_agent').then((r: any) => {
        setConnected(true)
        if (r?.sessions) setSessions(r.sessions)
        if (r?.sessions?.length > 0) setSessionId(r.sessions[0].seed || '')
      }).catch(() => setConnected(false))
    }).catch(() => {})
  }
  const handleDeleteSession = (seed: string) => {
    invoke('delete_session', { seed }).then(() => {
      refreshSessions()
      if (sessionId === seed) { setCacheInfo({ hit: 0, miss: 0 }); setTokenUsage(p => ({ ...p, used: 0 })); setMessages([]); setDocuments([]); setRecentEdits([]) }
    }).catch(() => {})
  }

  const newSession = () => {
    restartingRef.current = true
    setIsStreaming(false)
    setMessages([])
    setDocuments([])
    setRecentEdits([])
    streamRef.current = { content: '', reasoning: '', toolCards: [] }
    setSessionId('')
    setCacheInfo({ hit: 0, miss: 0 })
    setTokenUsage(p => ({ ...p, used: 0 }))
    invoke('stop_agent').then(() => {
      setConnected(false)
      invoke<any>('start_agent').then((r) => {
        setConnected(true)
        if (r?.sessions) setSessions(r.sessions)
        if (r?.sessions?.length > 0) setSessionId(r.sessions[0].seed || '')
        setTimeout(() => { restartingRef.current = false }, 1000)
        refreshSessions()
      }).catch(() => { setConnected(false); restartingRef.current = false })
    }).catch(() => { restartingRef.current = false })
  }

  const resumeSession = (seed: string) => {
    restartingRef.current = true
    setIsStreaming(false); setMessages([])
    setDocuments([])
    setRecentEdits([])
    streamRef.current = { content: '', reasoning: '', toolCards: [] }
    setSessionId(seed)
    setTokenUsage(p => ({ ...p, used: 0 }))
    setCacheInfo({ hit: 0, miss: 0 })
    invoke<any[]>('load_session_messages', { seed }).then(msgs => setMessages(msgs.map((m: any, i: number) => ({ ...m, _id: `loaded-${i}-${Date.now()}` })))).catch(() => {})
    invoke('resume_agent', { seed }).then(() => {
      setConnected(true)
      setTimeout(() => { restartingRef.current = false }, 1000)
      refreshSessions()
    }).catch((e: any) => {
      restartingRef.current = false
      pushMsg({ role: 'assistant', content: `\u26a0 ${e}` })
    })
  }

  const addTokens = useCallback((text: string, isUser: boolean) => {
    const tokens = Math.max(1, Math.ceil(text.length / 2))
    setTokenUsage(p => ({ ...p, used: p.used + tokens + (isUser ? 0 : tokens) }))
  }, [])

  useEffect(() => { chatEnd.current?.scrollIntoView({ behavior: 'auto' }) }, [messages, tick])

  useEffect(() => {
    invoke<boolean>('check_config').then(e => { setConfigDone(e); setChecking(false) }).catch(() => { setConfigDone(true); setChecking(false) })
  }, [])

  useEffect(() => {
    if (!configDone || connected || connectingRef.current) return
    connectingRef.current = true
    invoke<any[]>('cmd_sessions').then((existing) => {
      const latest = Array.isArray(existing) && existing.length > 0 ? existing[0].seed : null
      if (latest) {
        invoke<any>('resume_agent', { seed: latest }).then(() => {
          setConnected(true); setSessionId(latest)
          invoke<any[]>('load_session_messages', { seed: latest })
            .then(msgs => setMessages(msgs.map((m: any, i: number) => ({ ...m, _id: `loaded-${i}-${Date.now()}` })))).catch(() => {})
          invoke<any>('cmd_sessions').then(setSessions).catch(() => {})
        }).catch((e: any) => {
          setConnected(false)
          pushMsg({ role: 'assistant', content: `\u26a0 ${e}` })
        })
      } else {
        invoke<any>('start_agent').then((r: any) => {
          setConnected(true)
          if (r?.sessions) setSessions(r.sessions)
          if (r?.sessions?.length > 0) setSessionId(r.sessions[0].seed || '')
        }).catch((e: any) => {
          setConnected(false)
          pushMsg({ role: 'assistant', content: `\u26a0 ${e}` })
        })
      }
    }).catch(() => {})
  }, [configDone])

  useEffect(() => {
    const unlistens: Array<Promise<() => void>> = []

    unlistens.push(listen<any>('agent-event', (e: any) => {
      const p = e.payload
      const kind = p?.type as string
      switch (kind) {
        case 'stream_start': {
          setIsStreaming(true)
          const k = p.kind as string
          if (k === 'thinking') setStream('thinking')
          else if (k === 'tool_calling') { setStream('tool_calling'); if (p.tool_names) setToolNames(p.tool_names) }
          else setStream('answering')
          break
        }
        case 'stream_delta': {
          const delta = p.delta || ''
          if (streamModeRef.current === 'thinking') streamRef.current.reasoning += delta
          else streamRef.current.content += delta
          rerender()
          break
        }
        case 'stream_end': {
          // no-op — assistant_msg will follow with final content
          break
        }
        case 'assistant_msg': {
          const { thinking, text } = p
          const prevCards = [...streamRef.current.toolCards]
          streamRef.current = { content: '', reasoning: '', toolCards: [] }
          pushMsg({
            role: 'assistant' as const,
            content: text || '',
            reasoning: thinking || undefined,
            tool_cards: prevCards.length > 0 ? prevCards : undefined
          })
          setStream('idle')
          break
        }
        case 'tool_call': {
          const tool = p.tool || p
          const card = { id: tool.id, name: tool.name, args: tool.args_display || '', body: tool.body, output: '' }
          streamRef.current.toolCards.push(card)
          break
        }
        case 'tool_result': {
          const { tool_id, output, success } = p
          // Update tool card result
          const card = streamRef.current.toolCards.find(t => t.id === tool_id)
          if (card) { card.output = output || ''; card.success = success }
          // Also update in messages
          setMessages(prev => prev.map(msg => {
            if (!msg.tool_cards) return msg
            const hasMatch = msg.tool_cards.some((tc: any) => tc.id === tool_id)
            if (!hasMatch) return msg
            return {
              ...msg,
              tool_cards: msg.tool_cards.map((tc: any) =>
                tc.id === tool_id ? { ...tc, output: output || '' } : tc
              )
            }
          }))
          rerender()
          break
        }
        case 'turn_end': {
          const finalContent = streamRef.current.content
          const finalReasoning = streamRef.current.reasoning
          const finalCards = [...streamRef.current.toolCards]
          streamRef.current = { content: '', reasoning: '', toolCards: [] }
          if (finalContent || finalReasoning) {
            pushMsg({ role: 'assistant' as const, content: finalContent || '', reasoning: finalReasoning || undefined,
              tool_cards: finalCards.length > 0 ? finalCards : undefined })
          }
          if (p.usage) {
            setTokenUsage(prev => ({
              used: p.usage.prompt_tokens || 0,
              limit: prev.limit
            }))
            if (p.usage.prompt_cache_hit_tokens !== undefined || p.usage.prompt_cache_miss_tokens !== undefined) {
              setCacheInfo((c: { hit: number; miss: number }) => ({
                hit: c.hit + (p.usage.prompt_cache_hit_tokens || 0),
                miss: c.miss + (p.usage.prompt_cache_miss_tokens || 0)
              }))
            }
          }
          if (p.context_limit) {
            setTokenUsage(prev => ({ ...prev, limit: p.context_limit }))
          }
          fetchBalance()
          setIsStreaming(false)
          setStream('idle')
          rerender()
          break
        }
        case 'done': {
          setIsStreaming(false); setStream('idle'); rerender()
          break
        }
        case 'error': {
          setIsStreaming(false); setStream('idle'); rerender()
          break
        }
        case 'cancelled': {
          setIsStreaming(false); setStream('idle')
          streamRef.current = { content: '', reasoning: '', toolCards: [] }
          rerender()
          break
        }
        case 'ask_user': {
          setAskUser({ question: p.question || '需要输入', options: p.options })
          setAskAnswer('')
          break
        }
        case 'balance': {
          setBalance(`${p.total_balance || '0'} ${p.currency || 'CNY'}`)
          break
        }
        case 'session_restored': {
          setSessionId(p.seed || '')
          if (p.tokens_used) { setTokenUsage(prev => ({ ...prev, used: p.tokens_used })) }
          if (p.cache_hit_pct !== undefined && p.cache_hit_pct > 0) {
            const total = p.tokens_used || 1
            setCacheInfo({ hit: Math.round(total * p.cache_hit_pct / 100), miss: Math.round(total * (100 - p.cache_hit_pct) / 100) })
          }
          break
        }
        case 'debug_snapshot': {
          if (p.context_tokens) {
            setTokenUsage(prev => ({ ...prev, used: p.context_tokens, limit: p.context_limit || prev.limit }))
          }
          if (p.documents) setDocuments(p.documents)
          if (p.recent_edits) setRecentEdits(p.recent_edits)
          if (p.tasks) setTaskList(p.tasks)
          if (p.dsml_compat_count !== undefined) setDsmlCompat(p.dsml_compat_count)
          break
        }
        case 'shutdown_ack': {
          setIsStreaming(false)
          setStream('idle')
          break
        }
      }
    }))

    unlistens.push(listen('agent-closed', () => { if (!restartingRef.current) setConnected(false); setIsStreaming(false) }))

    unlistens.push(listen('agent-error', (e: any) => {
      const msg = (e.payload?.message || 'Agent error') as string
      pushMsg({ role: 'assistant', content: `\u26a0 ${msg}` })
      setIsStreaming(false); setStream('idle'); rerender()
    }))

    return () => { unlistens.forEach(p => p.then(fn => fn()).catch(() => {})) }
  }, [])

  useEffect(() => { if (connected) inputRef.current?.focus() }, [connected])

  const send = useCallback(() => {
    if (!input.trim() || isStreaming || !connected) return
    const text = input.trim(); setInput('')
    pushMsg({ role: 'user', content: text })
    addTokens(text, true)
    streamRef.current = { content: '', reasoning: '', toolCards: [] }
    setStream('thinking'); setIsStreaming(true)
    invoke('send_message', { text }).catch(() => setIsStreaming(false))
    setTimeout(() => inputRef.current?.focus(), 50)
  }, [input, isStreaming, connected, addTokens])

  const submitAskAnswer = useCallback(() => {
    if (!askUser) return
    const response = askAnswer.trim() || 'skipped'
    invoke('send_message', { text: response }).catch(() => {})
    setAskUser(null)
    setAskAnswer('')
  }, [askUser, askAnswer])

  if (checking) return <div className="h-screen flex items-center justify-center bg-[var(--bg-primary)]"><span className="text-[var(--muted)] text-sm">{T.loading}</span></div>
  if (!configDone) return <ConfigWizard onDone={() => { setConfigDone(true); setConfigVersion(v => v + 1) }} />

  return (
    <div className="h-screen flex flex-col bg-[var(--bg-primary)]">
      <div className="flex-1 flex min-h-0">
        <div className={`${leftOpen ? 'w-56' : 'w-0'} border-r border-[var(--border)] bg-[var(--bg-secondary)] flex-shrink-0 overflow-hidden transition-all duration-200`}>
          <div className="w-56">
          <InfoPanel
            tokens={tokenUsage} cache={cacheInfo}
            balance={balance} sessionId={sessionId} sessions={sessions}
            onSettings={() => setShowSettings(true)}
            onNewSession={newSession}
            onResumeSession={resumeSession}
            onDeleteAllSessions={handleDeleteAllSessions}
            onDeleteSession={handleDeleteSession}
          />
          </div>
        </div>
        <div className="flex-1 flex flex-col min-w-0">
          <div className="h-9 border-b border-[var(--border)] bg-[var(--bg-secondary)] flex items-center px-4 gap-2 text-xs text-[var(--muted)]">
            <button onClick={() => setLeftOpen(o => !o)} className="hover:text-[var(--text)] transition-colors" title={leftOpen ? '折叠侧栏' : '展开侧栏'}>
              {leftOpen ? '◀' : '▶'}
            </button>
            <span className={`w-2 h-2 rounded-full ${connected ? 'bg-[var(--success)]' : 'bg-[var(--warning)]'}`} />
            {connected ? T.hpConnected : T.connecting}
            <div className="flex-1" />
            <button onClick={() => setRightOpen(o => !o)} className="hover:text-[var(--text)] transition-colors" title={rightOpen ? '折叠侧栏' : '展开侧栏'}>
              {rightOpen ? '▶' : '◀'}
            </button>
            {isStreaming && (
              <button onClick={() => {
                setIsStreaming(false)
                pushMsg({ role: 'assistant', content: '⚠ 已终止操作' })
                invoke('cancel_agent').catch(() => {})
              }}
                className="px-2 py-0.5 rounded text-[13px] bg-[var(--error)]/10 text-[var(--error)] border border-[var(--error)]/30 hover:bg-[var(--error)]/20 transition-all">
                ■ 停止
              </button>
            )}
          </div>
          <div className="h-7 border-b border-[var(--border)] bg-[var(--bg-secondary)] flex items-center px-4 gap-3 text-xs text-[var(--muted)]">
            <span>📊 {tokenUsage.used.toLocaleString()} / {((tokenUsage.used / Math.max(tokenUsage.limit, 1)) * 100).toFixed(0)}%</span>
            {dsmlCompat > 0 && <span className="text-[var(--accent)]">DSML compat: {dsmlCompat}</span>}
          </div>
          <div className="flex-1 overflow-y-auto px-6 py-4">
            {messages.length === 0 && !isStreaming && (
              <div className="h-full flex items-center justify-center">
                <div className="text-center"><div className="text-3xl font-bold text-[var(--text-h)] mb-2">DSX</div><div className="text-sm text-[var(--muted)]">{T.welcome}</div></div>
              </div>
            )}
            {messages.map((msg, i) => <ChatMessage key={(msg as any)._id ?? i} msg={msg} />)}
            {isStreaming && (streamRef.current.content || streamRef.current.reasoning || streamRef.current.toolCards.length > 0) &&
              <ChatMessage msg={{
                role: 'assistant',
                content: streamRef.current.content || '',
                reasoning: streamRef.current.reasoning || undefined,
                tool_cards: streamRef.current.toolCards.length > 0 ? streamRef.current.toolCards : undefined
              }} />}
            {isStreaming && !streamRef.current.content && !streamRef.current.reasoning && !streamRef.current.toolCards.length && (
              <StreamIndicator mode={streamMode} toolNames={toolNames} secs={thinkingSecs} />
            )}
            <div ref={chatEnd} />
          </div>
          <div className="border-t border-[var(--border)] px-4 py-1.5 bg-[var(--bg-secondary)] flex items-center gap-2 text-[14px] text-[var(--muted)]">
            {modelOptions.length > 0 ? (
                <select value={configInfo.model || modelOptions[0]} onChange={e => {
                  invoke('update_config', { field: 'model', value: e.target.value }).catch(() => {})
                  invoke('reload_agent').catch(() => {})
                }}
                className="bg-[var(--bg-tertiary)] border border-[var(--border)] rounded px-1.5 py-0.5 text-[14px] text-[var(--accent)] font-mono outline-none cursor-pointer">
                {modelOptions.map(m => <option key={m} value={m}>{m}</option>)}
              </select>
            ) : (
              <span className="text-[var(--accent)] font-mono">{configInfo.model || 'deepseek-v4-flash'}</span>
            )}
            {configInfo.effort && <span>· 思考: {configInfo.effort === 'high' ? '高' : configInfo.effort === 'medium' ? '中' : configInfo.effort === 'low' ? '低' : configInfo.effort}</span>}
            <span className="flex-1" />
            <span className="font-mono">{sessionId ? `#${sessionId.slice(0, 8)}` : ''}</span>
          </div>
          <div className="border-t border-[var(--border)] p-4 bg-[var(--bg-secondary)]">
            <div className="max-w-4xl mx-auto flex gap-3">
              <textarea ref={inputRef} value={input} onChange={e => setInput(e.target.value)}
                onInput={(e: any) => { const el = e.currentTarget; el.style.height = 'auto'; el.style.height = el.scrollHeight + 'px' }}
                onKeyDown={(e: KeyboardEvent) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send() } }}
                placeholder={connected ? T.inputPlaceholder : T.connecting}
                disabled={isStreaming || !connected}
                rows={1}
                style={{ lineHeight: '1.5', position: 'relative' } as const}
                className="flex-1 bg-[var(--bg-primary)] border border-[var(--border)] rounded-xl px-4 py-3 text-sm text-[var(--text-h)] placeholder-[var(--muted)] outline-none focus:border-[var(--accent)] disabled:opacity-40 resize-none overflow-hidden" />
              <button onClick={send} disabled={isStreaming || !input.trim() || !connected}
                className="bg-[var(--accent)] text-white rounded-xl px-5 py-3 text-sm font-medium hover:brightness-110 disabled:opacity-40 disabled:cursor-not-allowed">{isStreaming ? '…' : '→'}</button>
            </div>
          </div>
        </div>
        <div className={`${rightOpen ? 'w-56' : 'w-0'} border-l border-[var(--border)] bg-[var(--bg-secondary)] flex-shrink-0 overflow-hidden transition-all duration-200`}>
          <div className="w-56">
          <WorkspacePanel documents={documents} recentEdits={recentEdits} tasks={taskList} />
          </div>
        </div>
      </div>
      {showSettings && <SettingsDialog onClose={() => { setShowSettings(false); setConfigVersion(v => v + 1) }} />}
      {askUser && <AskUserDialog question={askUser.question} options={askUser.options}
        answer={askAnswer} setAnswer={setAskAnswer} onSubmit={submitAskAnswer} />}
    </div>
  )
}