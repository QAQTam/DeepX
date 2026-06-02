import { memo, useState } from 'react'
import ReactMarkdown from 'react-markdown'
import type { Components } from 'react-markdown'
import remarkGfm from 'remark-gfm'
import rehypeHighlight from 'rehype-highlight'
import 'highlight.js/styles/github-dark.css'
import { T } from '../i18n'
import type { Message } from '../types'
import { execLiveOutput, toolResults } from '../state'

const mdComponents: Components = {
  table({ children }) {
    return <div className="overflow-x-auto my-2 border border-[var(--border)] rounded-lg"><table className="w-full text-xs border-collapse">{children}</table></div>
  },
  thead({ children }) { return <thead className="bg-[var(--bg-tertiary)]">{children}</thead> },
  th({ children }) { return <th className="border-b border-[var(--border)] px-3 py-2 text-left font-medium">{children}</th> },
  td({ children }) { return <td className="border-b border-[var(--border)] px-3 py-2">{children}</td> },
  tr({ children }) { return <tr className="even:bg-[var(--bg-tertiary)]/50">{children}</tr> },
  code({ className, children, ...props }: any) {
    const match = /language-(\w+)/.exec(className || '')
    return match ? (
      <code className={className} {...props}>{children}</code>
    ) : (
      <code className="bg-[var(--bg-tertiary)] px-1.5 py-0.5 rounded text-xs font-mono text-[var(--accent)]" {...props}>{children}</code>
    )
  },
  pre({ children }) { return <pre className="bg-[#0d1117] p-3 rounded-lg overflow-x-auto text-[12px] leading-relaxed my-2 border border-[var(--border)]">{children}</pre> },
  a({ children, href }) { return <a href={href} target="_blank" rel="noopener noreferrer" className="text-[var(--accent)] underline hover:opacity-80">{children}</a> },
  ul({ children }) { return <ul className="list-disc pl-5 my-1 space-y-0.5">{children}</ul> },
  ol({ children }) { return <ol className="list-decimal pl-5 my-1 space-y-0.5">{children}</ol> },
  del({ children }) { return <del className="text-[var(--muted)] line-through">{children}</del> },
  blockquote({ children }) { return <blockquote className="border-l-2 border-[var(--accent)] pl-3 my-2 italic text-[var(--muted)]">{children}</blockquote> },
  h1({ children }) { return <h1 className="text-lg font-bold my-3">{children}</h1> },
  h2({ children }) { return <h2 className="text-base font-bold my-2">{children}</h2> },
  h3({ children }) { return <h3 className="text-sm font-bold my-2">{children}</h3> },
  hr() { return <hr className="my-3 border-[var(--border)]" /> },
  p({ children }) { return <p className="my-1.5 last:mb-0">{children}</p> },
}

function StreamingMarkdown({ content }: { content: string }) {
  const cleaned = content.replace(/<\/?(?:th|td|tr|thead|tbody|table|colgroup|col|caption)(?:\s[^>]*)?>/gi, '')
  return (
    <div className="prose prose-sm max-w-none prose-invert">
      <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeHighlight]} components={mdComponents}>
        {cleaned}
      </ReactMarkdown>
    </div>
  )
}

const ChatMessage = memo(function ChatMessage({ msg }: { msg: Message }) {
  const [openSegments, setOpenSegments] = useState<Record<number, boolean>>({})
  const toggleSegment = (idx: number) => setOpenSegments(p => ({ ...p, [idx]: !(p[idx] ?? true) }))

  const timeline = (() => {
    if (msg.role !== 'assistant') return []
    const segs = msg.reasoningSegments || []
    const tools = msg.tool_calls || []
    const items: { kind: 'think' | 'tool'; content?: string; tool?: any }[] = []
    const len = Math.max(segs.length, tools.length)
    for (let i = 0; i < len; i++) {
      if (i < segs.length) items.push({ kind: 'think', content: segs[i] })
      if (i < tools.length) items.push({ kind: 'tool', tool: tools[i] })
    }
    if (items.length === 0 && msg.reasoning) items.push({ kind: 'think', content: msg.reasoning })
    return items
  })()

  return (
    <div className={`mb-4 ${msg.role === 'user' ? 'text-right' : ''}`}>
      {timeline.length > 0 && (
        <div className="mb-2 space-y-2">
          {timeline.map((item, idx) => {
            if (item.kind === 'think') {
              const open = openSegments[idx] ?? true
              return (
                <div key={idx}>
                  <button onClick={() => toggleSegment(idx)}
                    className="text-xs text-[var(--muted)] hover:text-[var(--accent)] transition-colors">
                    {open ? '▾' : '▸'} {open ? T.reasoningHide : `${T.reasoningShow} ${idx + 1}`}
                  </button>
                  {open && (
                    <div
                      className="text-xs text-[var(--muted)] bg-[var(--bg-tertiary)] rounded-lg p-3 border border-[var(--border)] whitespace-pre-wrap max-h-48 overflow-y-auto mt-1">
                      {item.content}
                    </div>
                  )}
                </div>
              )
            }
            return <ToolCard key={idx} tc={item.tool!} />
          })}
        </div>
      )}
      {msg.role === 'tool' ? (
        <ToolResultBubble name={msg.name || ''} content={msg.content || ''} />
      ) : (
        <div className={`inline-block max-w-[80%] rounded-2xl px-4 py-2.5 text-sm leading-relaxed ${
          msg.role === 'user'
            ? 'bg-[var(--accent)] text-white rounded-br-md shadow-sm'
            : 'bg-[var(--bg-secondary)] text-[var(--text-h)] border border-[var(--border)] rounded-bl-md'
        }`}>
          {msg.role === 'user' ? (
            <span className="whitespace-pre-wrap">{msg.content || '...'}</span>
          ) : (
            <StreamingMarkdown content={msg.content || ''} />
          )}
        </div>
      )}
    </div>
  )
})

function ToolResultBubble({ name, content }: { name: string; content: string }) {
  const lines = content.split('\n')
  const totalLines = lines.length
  const displayLines = lines.slice(0, 100)
  const isTruncated = totalLines > 100

  return (
    <div className="inline-block max-w-[80%] rounded-2xl border border-[var(--border)] overflow-hidden bg-[#0d1117] rounded-bl-md">
      <div className="flex items-center gap-1.5 px-3 py-1 border-b border-[#30363d] bg-[#161b22]">
        <span className="text-[10px] font-mono text-[var(--success)]">✓</span>
        <span className="text-[10px] font-mono text-[var(--muted)]">{name}</span>
        <span className="text-[10px] font-mono text-[var(--muted)]">· {totalLines} 行</span>
      </div>
      <div className="max-h-64 overflow-y-auto font-mono text-[11px] leading-relaxed text-[var(--text)] p-2">
        {displayLines.map((line, i) => (
          <div key={i} className="px-1 py-px">{line || '\u00A0'}</div>
        ))}
        {isTruncated && (
          <div className="text-[10px] text-[var(--muted)] italic mt-1 pt-1 border-t border-[#30363d]">
            ... 仅显示前 100 / {totalLines} 行
          </div>
        )}
      </div>
    </div>
  )
}

function ToolCard({ tc }: { tc: any }) {
  const isExec = tc.name === 'exec'
  const isEdit = tc.name === 'edit_file' || tc.name === 'edit_file_diff'
  const isReadWrite = tc.name === 'read_file' || tc.name === 'write_file'
  const isDiff = tc.name === 'diff'
  const isExplore = tc.name === 'explore'
  const isDir = tc.name === 'list_dir'
  const isSearch = tc.name === 'search'
  const isAsk = tc.name === 'ask_user' || tc.name === 'ask'
  const isTask = ['task_create', 'task_update', 'task_list'].includes(tc.name)
  const isWeb = ['web_fetch', 'web_search'].includes(tc.name)
  const isPlan = ['plan_create', 'plan_update', 'plan_read', 'plan_list'].includes(tc.name)
  const isMem = ['mem_save', 'mem_read', 'mem_forget', 'recall'].includes(tc.name)
  const isPitfall = ['pitfall_save', 'pitfall_guide'].includes(tc.name)
  const isGit = tc.name === 'git'

  return (
    <div className={`rounded-xl border border-[var(--border)] overflow-hidden ${
      isExec ? 'bg-[#0d1117] border-[#30363d]' : 'bg-[var(--bg-tertiary)]'
    }`}>
      <div className={`flex items-center gap-1.5 px-3 py-1.5 text-[11px] font-mono ${
        isExec ? 'bg-[#161b22] text-[#8b949e] border-b border-[#30363d]' : 'bg-[var(--bg-secondary)] text-[var(--muted)] border-b border-[var(--border)]'
      }`}>
        {isExec ? <span className="text-[var(--success)]">●</span>
          : isEdit || isDiff ? <span className="text-[var(--warning)]">△</span>
          : isGit ? <span className="text-[var(--warning)]">⎇</span>
          : <span className="text-[var(--accent)]">◇</span>}
        <span className={isExec ? 'text-[#e6edf3]' : 'text-[var(--text-h)]'}>{tc.name}</span>
        {tc.args && !isExec && <span className="truncate text-[var(--muted)]">{safeArgsStr(tc).slice(0, 90)}</span>}
      </div>

      {isExec && <ExecBody tc={tc} />}
      {isEdit && <DiffFileBody tc={tc} />}
      {isDiff && <DiffBody tc={tc} />}
      {isReadWrite && <KVBody tc={tc} />}
      {isExplore && <ExploreBody tc={tc} />}
      {isDir && <DirBody tc={tc} />}
      {isSearch && <SearchBody tc={tc} />}
      {isAsk && <AskBody tc={tc} />}
      {isTask && <KVBody tc={tc} icon="📋" />}
      {isWeb && <WebBody tc={tc} />}
      {isPlan && <KVBody tc={tc} icon="📋" />}
      {isMem && <MemBody tc={tc} />}
      {isPitfall && <PitfallBody tc={tc} />}
      {isGit && <GitBody tc={tc} />}
      {!isExec && !isEdit && !isDiff && !isReadWrite && !isExplore && !isDir && !isSearch && !isAsk && !isTask && !isWeb && !isPlan && !isMem && !isPitfall && !isGit && (
        <KVBody tc={tc} />
      )}

      <ToolResult tc={tc} />
    </div>
  )
}

function ExecBody({ tc }: { tc: any }) {
  const p = parseArgs(tc)
  const cmd = p ? String(p.command || p.cmd || '') : safeArgsStr(tc)
  return (
    <div className="px-3 py-2 font-mono text-[12px] leading-relaxed text-[#e6edf3]">
      <div className="flex items-center gap-2 text-[#8b949e] mb-1">
        <span>$</span>
        <span>{cmd}</span>
      </div>
      {(() => {
        const live = execLiveOutput[tc.id || ''] || execLiveOutput[tc.name]
        const display = live || tc.output
        return display ? (
          <div className="whitespace-pre-wrap text-[12px] text-[#e6edf3] max-h-60 overflow-y-auto mt-2">
            <ResultViewer content={display} toolName={tc.name} maxLines={200} />
          </div>
        ) : (
          <div className="text-[11px] text-[#8b949e] mt-2">等待输出...</div>
        )
      })()}
    </div>
  )
}

function safeArgsStr(tc: any): string {
  if (typeof tc.args === 'string') return tc.args
  if (tc.args && typeof tc.args === 'object') return JSON.stringify(tc.args)
  return ''
}

function parseArgs(tc: any): Record<string, unknown> | null {
  const a = tc.args
  if (a && typeof a === 'object' && !Array.isArray(a)) return a as Record<string, unknown>
  if (typeof a === 'string') { try { return JSON.parse(a) } catch { return null } }
  return null
}

function KVBody({ tc, icon }: { tc: any; icon?: string }) {
  const p = parseArgs(tc)
  if (!p) return <div className="px-3 py-2 text-[12px] font-mono text-[var(--muted)]">{safeArgsStr(tc).slice(0, 80)}</div>
  return (
    <div className="px-3 py-2 text-[12px] font-mono text-[var(--text)]">
      {icon && <div className="mb-1">{icon}</div>}
      {Object.entries(p).map(([k, v]) => (
        <div key={k} className="flex gap-2"><span className="text-[var(--muted)]">{k}:</span><span className="text-[var(--text-h)]">{String(v).slice(0, 60)}</span></div>
      ))}
    </div>
  )
}

function DiffFileBody({ tc }: { tc: any }) {
  const p = parseArgs(tc)
  if (!p) return <div className="px-3 py-2 text-[12px] font-mono text-[var(--muted)]">{safeArgsStr(tc).slice(0, 80)}</div>
  const file = (p.path || p.file_path) as string || ''
  const oldStr = (p.old_string || p.old_str) as string || ''
  const newStr = (p.new_string || p.new_str) as string || ''
  const oldLines = (p.old_lines as string[]) || (oldStr ? oldStr.split('\n') : [])
  const newLines = (p.new_lines as string[]) || (newStr ? newStr.split('\n') : [])
  return (
    <div className="px-3 py-2 text-[12px] font-mono">
      {file && <div className="text-[var(--accent)] font-bold mb-1 text-[11px]">{file}</div>}
      {oldLines.length > 0 || newLines.length > 0 ? (
        <div className="grid grid-cols-2 gap-x-4 gap-y-0 text-[11px] leading-snug">
          <div className="text-[var(--muted)] text-[10px] mb-0.5">— 旧</div>
          <div className="text-[var(--muted)] text-[10px] mb-0.5">+ 新</div>
          {Array.from({ length: Math.max(oldLines.length, newLines.length, 1) }).map((_, i) => (
            <>
              <div key={`old-${i}`} className="text-red-400/80 bg-red-500/5 border-l-2 border-red-500/30 pl-1.5 truncate">{oldLines[i] || '\u00A0'}</div>
              <div key={`new-${i}`} className="text-green-400/80 bg-green-500/5 border-l-2 border-green-500/30 pl-1.5 truncate">{newLines[i] || '\u00A0'}</div>
            </>
          ))}
        </div>
      ) : (
        <div className="text-[var(--muted)] text-[11px]">{safeArgsStr(tc).slice(0, 120)}</div>
      )}
    </div>
  )
}

function DiffBody({ tc }: { tc: any }) {
  const p = parseArgs(tc)
  return (
    <div className="px-3 py-2 text-[12px] font-mono text-[var(--text)]">
      <div className="flex gap-4 text-[11px]">
        {p?.path_a ? <span className="text-red-400/80">- {String(p.path_a)}</span> : null}
        {p?.path_b ? <span className="text-green-400/80">+ {String(p.path_b)}</span> : null}
      </div>
      {(!p?.path_a && !p?.path_b) && <span className="text-[var(--muted)]">{safeArgsStr(tc).slice(0, 80)}</span>}
    </div>
  )
}

function DirBody({ tc }: { tc: any }) {
  const p = parseArgs(tc)
  return (
    <div className="px-3 py-2 text-[12px] font-mono text-[var(--text)]">
      <span>{p && p.path ? `📁 ${String(p.path)}` : `📁 ${safeArgsStr(tc).slice(0, 60)}`}</span>
    </div>
  )
}

function ExploreBody({ tc }: { tc: any }) {
  const p = parseArgs(tc)
  return (
    <div className="px-3 py-2 text-[12px] font-mono text-[var(--text)]">
      <span className="text-[var(--accent)]">🔍 {p && (p.path || p.directory) ? String(p.path || p.directory) : safeArgsStr(tc).slice(0, 60)}</span>
      {p?.depth !== undefined ? <span className="text-[var(--muted)] ml-2">depth={String(p.depth)}</span> : null}
    </div>
  )
}

function SearchBody({ tc }: { tc: any }) {
  const p = parseArgs(tc)
  return (
    <div className="px-3 py-2 text-[12px] font-mono text-[var(--text)]">
      <span>🔍 <span className="text-[var(--accent)]">{p && p.pattern ? String(p.pattern) : safeArgsStr(tc).slice(0, 60)}</span></span>
    </div>
  )
}

function AskBody({ tc }: { tc: any }) {
  const p = parseArgs(tc)
  return (
    <div className="px-3 py-2 text-[12px] text-[var(--text-h)]">
      <span>❓ {p && p.question ? String(p.question) : safeArgsStr(tc).slice(0, 80)}</span>
    </div>
  )
}

function WebBody({ tc }: { tc: any }) {
  const p = parseArgs(tc)
  return (
    <div className="px-3 py-2 text-[12px] font-mono text-[var(--text)]">
      <span>🌐 {p && (p.url || p.query) ? String(p.url || p.query) : safeArgsStr(tc).slice(0, 80)}</span>
    </div>
  )
}

function MemBody({ tc }: { tc: any }) {
  const p = parseArgs(tc)
  return (
    <div className="px-3 py-2 text-[12px] font-mono text-[var(--text)]">
      <span>🧠 {p && (p.name || p.key) ? String(p.name || p.key) : safeArgsStr(tc).slice(0, 80)}</span>
    </div>
  )
}

function PitfallBody({ tc }: { tc: any }) {
  const p = parseArgs(tc)
  return (
    <div className="px-3 py-2 text-[12px] font-mono text-[var(--text)]">
      <span>⚠️ {p && p.issue ? String(p.issue) : safeArgsStr(tc).slice(0, 80)}</span>
    </div>
  )
}

function GitBody({ tc }: { tc: any }) {
  const p = parseArgs(tc)
  const action = (p && (p.action || p.subcommand)) ? String(p.action || p.subcommand) : safeArgsStr(tc).slice(0, 80)
  return (
    <div className="px-3 py-2 text-[12px] font-mono text-[var(--text)]">
      <span className="text-[var(--warning)]">⎇ {action}</span>
      {p?.message ? <span className="text-[var(--muted)] ml-2">{String(p.message).slice(0, 60)}</span> : null}
      {p?.path ? <span className="text-[var(--muted)] ml-2">{String(p.path).slice(0, 40)}</span> : null}
    </div>
  )
}

function toolResultContent(tc: any): { content: string; success: boolean } | null {
  return (tc.output != null && tc.output !== ''
    ? { content: String(tc.output), success: true }
    : null) || toolResults[tc.id || ''] || toolResults[tc.name]
}

function ToolResult({ tc }: { tc: any }) {
  const r = toolResultContent(tc)
  if (!r) return null
  const lines = r.content.split('\n')
  const totalLines = lines.length

  const truncated = lines.filter(l =>
    l.includes('[truncated:') || l.includes('more entries') || l.includes('more matches') ||
    l.includes('TRUNCATED:') || l.includes('more errors') || l.includes('more warnings') ||
    l.includes('similar lines')
  )
  const hasTruncation = truncated.length > 0

  return (
    <details className="group border-t border-[var(--border)]">
      <summary className="flex items-center gap-1.5 px-3 py-1 text-[10px] font-mono text-[var(--muted)] cursor-pointer hover:text-[var(--accent)] select-none">
        <span className="group-open:rotate-90 transition-transform">▸</span>
        <span className={r.success ? 'text-[var(--success)]' : 'text-[var(--error)]'}>{r.success ? '✓' : '✗'}</span>
        <span>{r.success ? '成功' : '失败'} · {totalLines} 行</span>
        {hasTruncation && <span className="text-[var(--warning)]">⚠ 截断</span>}
      </summary>
      <div className="max-h-64 overflow-y-auto bg-[var(--bg-tertiary)]/50 text-[var(--text)]">
        <ResultViewer content={r.content} toolName={tc.name} maxLines={300} />
        {hasTruncation && (
          <div className="px-3 py-1.5 text-[10px] font-mono text-[var(--warning)]/80 bg-[var(--warning)]/5 border-t border-[var(--border)]">
            ⚠ 输出已被截断，使用 read_file / exec 查看完整内容
          </div>
        )}
      </div>
    </details>
  )
}

function ResultViewer({ content, toolName, maxLines = 500 }: { content: string; toolName: string; maxLines?: number }) {
  const allLines = content.split('\n')
  const lines = allLines.slice(0, maxLines)
  const showing = lines.length
  const total = allLines.length

  return (
    <div className="font-mono leading-relaxed">
      {lines.map((line, i) => {
        const trimmed = line.trimStart()
        let cls = ''

        if (trimmed.startsWith('- ') || line.startsWith('  -   ')) {
          cls = 'bg-red-500/10 border-l-2 border-red-500/50'
        } else if (trimmed.startsWith('+ ') || line.startsWith('  +   ')) {
          cls = 'bg-green-500/10 border-l-2 border-green-500/50'
        } else if (/^@@\s/.test(trimmed)) {
          cls = 'text-blue-400 bg-blue-500/5'
        } else if (line.startsWith('[OK]')) {
          cls = 'text-green-400'
        } else if (line.startsWith('[FAIL]') || line.startsWith('[CANCELLED]')) {
          cls = 'text-red-400 font-bold'
        } else if (line.startsWith('[PARTIAL]')) {
          cls = 'text-amber-400'
        } else if (line.startsWith('[ERROR]')) {
          cls = 'text-red-400'
        } else if (line.startsWith('[HINT]')) {
          cls = 'text-slate-400 italic'
        } else if (line.startsWith('[CHANGE]')) {
          cls = 'text-green-400'
        } else if (line.startsWith('[PROJECT_MAP]')) {
          cls = 'text-blue-400 font-bold'
        } else if (line.startsWith('## ')) {
          cls = 'text-blue-400 font-bold text-[12px]'
        } else if (line.startsWith('[stdout]') || line.startsWith('[stderr]')) {
          cls = 'text-slate-400 text-[10px] uppercase tracking-wider'
        } else if (line.startsWith('──') || line.startsWith('──')) {
          cls = 'text-slate-500'
        } else if (line.startsWith('... ') && (line.includes('more') || line.includes('truncated') || line.includes('TRUNCATED'))) {
          cls = 'text-[var(--warning)]/70 text-[10px]'
        } else if (trimmed === '⚠ fuzzy match' || trimmed.startsWith('⚠ ')) {
          cls = 'text-amber-400'
        } else if (toolName === 'git' && /^[? ]?[MADR?] /.test(trimmed)) {
          const isUntracked = trimmed.startsWith('??')
          cls = isUntracked ? 'text-red-400 bg-red-500/5' : 'text-yellow-400'
        } else if (toolName === 'explore') {
          if (trimmed.endsWith('/') && !trimmed.includes(' ')) {
            cls = 'text-amber-300 font-bold'
          } else if (trimmed.startsWith('⇐') || trimmed.startsWith('→')) {
            cls = 'text-blue-400/70'
          }
        } else if (toolName === 'exec') {
          if (line.startsWith('$ ')) cls = 'text-green-400'
        }

        return (
          <div key={i} className={`px-2 py-px text-[11px] ${cls}`}>
            {line || '\u00A0'}
          </div>
        )
      })}
      {total > showing && (
        <div className="px-2 py-1 text-[10px] text-[var(--muted)] italic border-t border-[var(--border)]">
          ... 仅显示前 {showing} / {total} 行
        </div>
      )}
    </div>
  )
}

export { ChatMessage }
