// ── ToolCard ──
// Renders a tool call card by dispatching through ToolRegistry.

import { createSignal } from 'solid-js'
import { tt } from '../../i18n'
import { getToolRenderer, type ToolCardContext } from '../../domain/tool-registry'

interface ToolCardProps {
  ctx: ToolCardContext
}

export function ToolCard({ ctx }: ToolCardProps) {
  const [open, setOpen] = createSignal(false)
  const renderer = getToolRenderer(ctx.name)

  return (
    <div class="my-2 border border-[var(--border-light)] rounded-lg overflow-hidden bg-[var(--bg-surface)] transition-theme">
      {/* Header */}
      <button
        onClick={() => setOpen(o => !o)}
        class="w-full flex items-center gap-2 px-3 py-2 text-xs hover:bg-[var(--bg-tertiary)] transition-colors text-left"
        aria-expanded={open()}
      >
        <span class="shrink-0 text-sm" aria-hidden="true">
          {renderer?.icon || '⚙'}
        </span>
        <span class="font-medium text-[var(--text-h)]">
          {renderer?.label || ctx.name}
        </span>
        <span class="flex-1 text-[var(--text)] truncate">
          {renderer?.renderHeader(ctx)}
        </span>
        <span class={`shrink-0 text-[10px] px-1.5 py-0.5 rounded font-mono
          ${ctx.success === true ? 'bg-[var(--success-light)] text-[var(--success)]'
          : ctx.success === false ? 'bg-[var(--error-light)] text-[var(--error)]'
          : ctx.output !== undefined ? 'bg-[var(--success-light)] text-[var(--success)]'
          : 'bg-[var(--warning-light)] text-[var(--warning)]'}`}>
          {ctx.output !== undefined ? tt('chat.toolDone') : tt('chat.toolPending')}
        </span>
        <span class="shrink-0 text-[var(--muted)] text-xs">{open() ? '▾' : '▸'}</span>
      </button>

      {/* Body */}
      {open() && ctx.output !== undefined && (
        <div class="border-t border-[var(--border-light)]">
          {renderer?.renderResult ? (
            renderer.renderResult(ctx.output)
          ) : (
            <ToolResultOutput raw={ctx.output} />
          )}
        </div>
      )}
    </div>
  )
}

// ── Default tool result renderer (with color coding) ──

function ToolResultOutput({ raw }: { raw: string }) {
  const lines = raw.split('\n')
  const truncated = lines.length > 80
  const display = lines.slice(0, 80)

  return (
    <div class="p-3 text-xs font-mono max-h-64 overflow-y-auto">
      {display.map((line) => (
        <div class={lineColor(line)}>{line || ' '}</div>
      ))}
      {truncated && (
        <div class="text-[var(--warning)] mt-1 font-medium">
          ⚠ 输出被截断（共 {lines.length} 行）
        </div>
      )}
    </div>
  )
}

function lineColor(line: string): string {
  const trimmed = line.trimStart()
  if (trimmed.startsWith('[OK]') || trimmed.startsWith('[SUCCESS]')) return 'text-[var(--success)]'
  if (trimmed.startsWith('[ERROR]') || trimmed.startsWith('[FAIL')) return 'text-[var(--error)]'
  if (trimmed.startsWith('[WARN') || trimmed.startsWith('[HINT]')) return 'text-[var(--warning)]'
  if (trimmed.startsWith('[INFO]') || trimmed.startsWith('#')) return 'text-[var(--muted)]'
  if (trimmed.startsWith('$ ') || trimmed.startsWith('> ')) return 'text-[var(--text)] font-semibold'
  return 'text-[var(--text)]'
}
