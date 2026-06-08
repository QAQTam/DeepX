// ── Tool Card Registry (Strategy Pattern) ──
// Each tool registers its rendering strategy once.
// ToolCard.tsx dispatches by tool name through this registry.

import type { JSX } from 'solid-js'
import { createEffect } from 'solid-js'
import { tt } from '../i18n'

export interface ToolCardContext {
  id: string
  name: string
  args: string
  body?: unknown
  output?: string
  liveOutput?: string
  success?: boolean
}

export interface ToolRenderer {
  toolName: string | string[]
  icon: string
  label: string
  autoOpen?: boolean
  renderHeader: (ctx: ToolCardContext) => JSX.Element
  renderResult?: (output: string) => JSX.Element
}

const registry = new Map<string, ToolRenderer>()

export function registerTool(renderer: ToolRenderer) {
  const names = Array.isArray(renderer.toolName) ? renderer.toolName : [renderer.toolName]
  for (const name of names) {
    registry.set(name, renderer)
  }
}

export function getToolRenderer(name: string): ToolRenderer | undefined {
  return registry.get(name)
}

export function getAllToolLabels(): Record<string, string> {
  const labels: Record<string, string> = {}
  for (const [name, r] of registry) {
    labels[name] = r.label
  }
  return labels
}

// ── Register built-in tools ──

function sp(s: string): string {
  const seg = s.split(/[/\\]/).pop() || s
  return seg.length > 30 ? seg.slice(0, 28) + '…' : seg
}

function parseArgs(args: string): Record<string, string> {
  try { return JSON.parse(args) } catch { return {} }
}

registerTool({
  toolName: 'read_file',
  icon: '📄',
  label: '读取文件',
  autoOpen: false,
  renderHeader: ctx => {
    const a = parseArgs(ctx.args)
    return <span>{sp(a.path || '')}</span>
  },
  renderResult: output => <CodeBlock code={stripStatus(output)} />,
})

registerTool({
  toolName: 'write_file',
  icon: '✏️',
  label: '写入文件',
  renderHeader: ctx => {
    const a = parseArgs(ctx.args)
    return <span>{sp(a.path || '')}</span>
  },
  renderResult: output => <WriteResult output={output} />,
})

registerTool({
  toolName: 'edit_file',
  icon: '≪≫',
  label: '编辑文件',
  renderHeader: ctx => {
    const a = parseArgs(ctx.args)
    return <span>{sp(a.path || '')}</span>
  },
  renderResult: output => <EditResult output={output} />,
})

registerTool({
  toolName: 'edit_file_diff',
  icon: '≪≫',
  label: '模糊编辑',
  renderHeader: ctx => {
    const a = parseArgs(ctx.args)
    return <span>{sp(a.path || '')}</span>
  },
  renderResult: output => <EditResult output={output} />,
})

registerTool({
  toolName: ['delete_file', 'file_delete'],
  icon: '🗑',
  label: tt('tools.delete_file'),
  renderHeader: ctx => {
    const a = parseArgs(ctx.args)
    return <span>{sp(a.path || '')}</span>
  },
})

registerTool({
  toolName: ['move_file', 'file_move'],
  icon: '↗',
  label: '移动',
  renderHeader: ctx => {
    const a = parseArgs(ctx.args)
    return <span>{sp(a.path || '')} → {sp(a.dest || '')}</span>
  },
})

registerTool({
  toolName: 'file_copy',
  icon: '📋',
  label: tt('tools.copy_file'),
  renderHeader: ctx => {
    const a = parseArgs(ctx.args)
    return <span>{sp(a.path || '')} → {sp(a.dest || '')}</span>
  },
})

registerTool({
  toolName: 'exec',
  icon: '>_',
  label: '执行命令',
  autoOpen: false,
  renderHeader: ctx => {
    const a = parseArgs(ctx.args)
    const cwd = a.cwd
    return (
      <div class="font-mono text-xs">
        {cwd && <span class="text-[var(--muted)]">{sp(cwd)}</span>}
        {a.command && <span class="ml-1">{a.command.slice(0, 80)}</span>}
      </div>
    )
  },
  renderResult: output => <TerminalBlock output={output} />,
})

registerTool({
  toolName: 'explore',
  icon: '🔍',
  label: '探索项目',
  renderHeader: ctx => {
    const a = parseArgs(ctx.args)
    return <span>{a.path || '.'}</span>
  },
})

registerTool({
  toolName: ['list_dir', 'file_list_dir'],
  icon: '📂',
  label: '列出目录',
  renderHeader: ctx => {
    const a = parseArgs(ctx.args)
    return <span>{a.path || '.'}</span>
  },
})

registerTool({
  toolName: 'file_search',
  icon: '🔎',
  label: tt('tools.file_search'),
  renderHeader: ctx => {
    const a = parseArgs(ctx.args)
    return <span class="font-mono">{a.pattern || ''}</span>
  },
})

registerTool({
  toolName: 'file_glob',
  icon: '🔎',
  label: '查找文件',
  renderHeader: ctx => {
    const a = parseArgs(ctx.args)
    return <span class="font-mono">{a.pattern || ''}</span>
  },
})

registerTool({
  toolName: ['diff', 'file_diff'],
  icon: '⤻',
  label: '对比文件',
  renderHeader: ctx => {
    const a = parseArgs(ctx.args)
    return <span>{sp(a.path_a || '')} ↔ {sp(a.path_b || '')}</span>
  },
})

registerTool({
  toolName: ['task_create', 'task_update', 'task_list'],
  icon: '📋',
  label: '任务',
  renderHeader: ctx => {
    const a = parseArgs(ctx.args)
    return <span>{a.subject || a.description?.slice(0, 40) || ''}</span>
  },
})

registerTool({
  toolName: ['web_fetch', 'web_search'],
  icon: '🌐',
  label: '网页',
  renderHeader: ctx => {
    const a = parseArgs(ctx.args)
    return <span class="truncate max-w-[200px]">{a.url || a.query || ''}</span>
  },
})

registerTool({
  toolName: 'context7_query',
  icon: '📖',
  label: '查询文档',
  renderHeader: ctx => {
    const a = parseArgs(ctx.args)
    return <span>{a.query || ''}</span>
  },
})

registerTool({
  toolName: 'context7_resolve',
  icon: '📖',
  label: '解析库名',
  renderHeader: ctx => {
    const a = parseArgs(ctx.args)
    return <span>{a.name || ''}</span>
  },
})

registerTool({
  toolName: 'ask_user',
  icon: '❓',
  label: '询问',
  renderHeader: ctx => {
    const a = parseArgs(ctx.args)
    return <span>{a.question?.slice(0, 60) || ''}</span>
  },
})

// ── Custom Result Renderers ──

function stripStatus(raw: string): string {
  const lines = raw.split('\n')
  if (lines.length > 0 && (lines[0].startsWith('[OK]') || lines[0].startsWith('[ERROR]'))) return lines.slice(1).join('\n')
  return raw
}

function CodeBlock(props: { code: string; maxLines?: number }) {
  const lines = props.code.split('\n')
  const truncated = props.maxLines && lines.length > props.maxLines
  const shown = truncated ? lines.slice(0, props.maxLines) : lines
  return (
    <div class="rounded-lg overflow-hidden border border-[var(--border)]">
      <div class="px-3 py-1.5 bg-[#1e1e2e] text-[11px] text-[var(--muted)] font-mono flex items-center justify-between">
        <span>{lines.length} lines</span>
        <span class="text-[10px] opacity-50">plain text</span>
      </div>
      <pre class="!m-0 p-3 text-xs font-mono bg-[#1a1a2e] text-[#cdd6f4] overflow-x-auto max-h-96 leading-relaxed">
        <code>{shown.map((l, i) => (
          <div class="flex">
            <span class="shrink-0 w-10 text-right text-[#585b70] select-none mr-3">{i + 1}</span>
            <span class="whitespace-pre">{l || ' '}</span>
          </div>
        ))}</code>
        {truncated && <div class="text-[var(--warning)] mt-1 text-center">— {lines.length - props.maxLines!} more lines —</div>}
      </pre>
    </div>
  )
}

function WriteResult(props: { output: string }) {
  const firstLine = props.output.split('\n')[0]
  const isOk = firstLine.startsWith('[OK]')
  const content = stripStatus(props.output)
  return (
    <div>
      <div class={`px-3 py-2 text-xs font-mono rounded-t-lg flex items-center gap-2 ${isOk ? 'bg-[var(--success)]/10 text-[var(--success)]' : 'bg-[var(--error)]/10 text-[var(--error)]'}`}>
        <span>{isOk ? '✓' : '✗'}</span>
        <span>{firstLine}</span>
      </div>
      {content.trim() && <CodeBlock code={content} maxLines={30} />}
    </div>
  )
}

function EditResult(props: { output: string }) {
  const lines = props.output.split('\n')
  const status = lines[0] || ''
  const oldLine = lines.find(l => l.startsWith('old:'))?.replace('old: ', '') || ''
  const newLine = lines.find(l => l.startsWith('new:'))?.replace('new: ', '') || ''
  return (
    <div class="px-3 py-2 space-y-1.5">
      <div class={`text-xs font-mono ${status.startsWith('[OK]') ? 'text-[var(--success)]' : 'text-[var(--error)]'}`}>{status}</div>
      {oldLine && (
        <div class="flex items-start gap-2 text-xs font-mono">
          <span class="shrink-0 text-[var(--error)] font-medium">−</span>
          <code class="px-1.5 py-0.5 rounded bg-[var(--error)]/10 text-[var(--error)] break-all">{oldLine}</code>
        </div>
      )}
      {newLine && (
        <div class="flex items-start gap-2 text-xs font-mono">
          <span class="shrink-0 text-[var(--success)] font-medium">+</span>
          <code class="px-1.5 py-0.5 rounded bg-[var(--success)]/10 text-[var(--success)] break-all">{newLine}</code>
        </div>
      )}
    </div>
  )
}

function TerminalBlock(props: { output: string }) {
  let preRef!: HTMLPreElement
  createEffect(() => {
    props.output
    if (preRef) preRef.scrollTop = preRef.scrollHeight
  })
  return (
    <div class="rounded-lg overflow-hidden border border-[var(--border)]">
      <div class="px-3 py-1.5 bg-[#11111b] text-[11px] text-[var(--muted)] font-mono flex items-center gap-1.5">
        <span class="w-2 h-2 rounded-full bg-[#f38ba8]" />
        <span class="w-2 h-2 rounded-full bg-[#f9e2af]" />
        <span class="w-2 h-2 rounded-full bg-[#a6e3a1]" />
        <span class="ml-2 text-[10px]">terminal</span>
      </div>
      <pre ref={preRef} class="!m-0 p-3 text-xs font-mono bg-[#0d0d1a] text-[#cdd6f4] overflow-x-auto max-h-64 leading-relaxed whitespace-pre">{props.output}</pre>
    </div>
  )
}
