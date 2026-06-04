// ── Tool Card Registry (Strategy Pattern) ──
// Each tool registers its rendering strategy once.
// ToolCard.tsx dispatches by tool name through this registry.

import type { ReactNode } from 'react'

export interface ToolCardContext {
  id: string
  name: string
  args: string
  body?: unknown
  output?: string
  success?: boolean
}

export interface ToolRenderer {
  toolName: string | string[]  // single or aliases
  icon: string
  label: string
  renderHeader: (ctx: ToolCardContext) => ReactNode
  renderResult?: (output: string) => ReactNode  // optional custom result renderer
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
  renderHeader: ctx => {
    const a = parseArgs(ctx.args)
    return <span>{sp(a.path || '')}</span>
  },
})

registerTool({
  toolName: 'write_file',
  icon: '✏️',
  label: '写入文件',
  renderHeader: ctx => {
    const a = parseArgs(ctx.args)
    return <span>{sp(a.path || '')}</span>
  },
})

registerTool({
  toolName: 'edit_file',
  icon: '≪≫',
  label: '编辑文件',
  renderHeader: ctx => {
    const a = parseArgs(ctx.args)
    return <span>{sp(a.path || '')}</span>
  },
})

registerTool({
  toolName: 'edit_file_diff',
  icon: '≪≫',
  label: '模糊编辑',
  renderHeader: ctx => {
    const a = parseArgs(ctx.args)
    return <span>{sp(a.path || '')}</span>
  },
})

registerTool({
  toolName: ['delete_file', 'file_delete'],
  icon: '🗑',
  label: '删除',
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
  label: '复制',
  renderHeader: ctx => {
    const a = parseArgs(ctx.args)
    return <span>{sp(a.path || '')} → {sp(a.dest || '')}</span>
  },
})

registerTool({
  toolName: 'exec',
  icon: '>_',
  label: '执行命令',
  renderHeader: ctx => {
    const a = parseArgs(ctx.args)
    const cwd = a.cwd
    return (
      <div className="font-mono text-xs">
        {cwd && <span className="text-[var(--muted)]">{sp(cwd)}</span>}
        {a.command && <span className="ml-1">{a.command.slice(0, 80)}</span>}
      </div>
    )
  },
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
  label: '搜索内容',
  renderHeader: ctx => {
    const a = parseArgs(ctx.args)
    return <span className="font-mono">{a.pattern || ''}</span>
  },
})

registerTool({
  toolName: 'file_glob',
  icon: '🔎',
  label: '查找文件',
  renderHeader: ctx => {
    const a = parseArgs(ctx.args)
    return <span className="font-mono">{a.pattern || ''}</span>
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
    return <span className="truncate max-w-[200px]">{a.url || a.query || ''}</span>
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


