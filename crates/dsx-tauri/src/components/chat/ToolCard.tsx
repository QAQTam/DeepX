// ── ToolCard ──
// Renders a tool call card by dispatching through ToolRegistry.
// Pending cards (tool executing) get a running pulse animation.

import { createSignal, For } from 'solid-js'
import { tt } from '../../i18n'
import { getToolRenderer, type ToolCardContext } from '../../domain/tool-registry'

interface ToolCardProps {
  ctx: ToolCardContext
}

export function ToolCard(props: ToolCardProps) {
  const [open, setOpen] = createSignal(false)
  const renderer = getToolRenderer(props.ctx.name)
  const isPending = () => props.ctx.output === undefined

  return (
    <div
      class={`my-2 border rounded-lg overflow-hidden bg-[var(--bg-surface)] transition-theme animate-tool-card
        ${isPending()
          ? 'border-[var(--accent)]/30 shadow-[0_0_8px_rgba(124,58,237,0.12)] animate-tool-run'
          : 'border-[var(--border-light)]'}`}
    >
      {/* Header */}
      <button
        onClick={() => setOpen(o => !o)}
        class="w-full flex items-center gap-2 px-3 py-2 text-xs hover:bg-[var(--bg-tertiary)] transition-colors text-left"
        aria-expanded={open()}
      >
        <span class={`shrink-0 text-sm ${isPending() ? 'anim-spin-slow' : ''}`} aria-hidden="true">
          {renderer?.icon || '⚙'}
        </span>
        <span class="font-medium text-[var(--text-h)]">
          {renderer?.label || props.ctx.name}
        </span>
        <span class="flex-1 text-[var(--text)] truncate">
          {renderer?.renderHeader(props.ctx)}
        </span>
        {isPending() ? (
          <span class="shrink-0 flex items-center gap-1 text-[11px] font-mono px-1.5 py-0.5 rounded bg-[var(--accent)]/10 text-[var(--accent)] animate-task-pulse">
            <span class="inline-block w-1.5 h-1.5 rounded-full bg-[var(--accent)] animate-task-pulse" />
            {tt('chat.toolPending')}
          </span>
        ) : (
          <span class={`shrink-0 text-[11px] px-1.5 py-0.5 rounded font-mono
            ${props.ctx.success === true ? 'bg-[var(--success-light)] text-[var(--success)]'
            : props.ctx.success === false ? 'bg-[var(--error-light)] text-[var(--error)]'
            : 'bg-[var(--success-light)] text-[var(--success)]'}`}>
            {tt('chat.toolDone')}
          </span>
        )}
        <span class="shrink-0 text-[var(--muted)] text-xs">{open() ? '▾' : '▸'}</span>
      </button>

      {/* Body */}
      {open() && !isPending() && (
        <div class="border-t border-[var(--border-light)]">
          {renderer?.renderResult ? (
            renderer.renderResult(props.ctx.output ?? '')
          ) : (
            <ToolResultOutput raw={props.ctx.output ?? ''} />
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
      <For each={display}>{(line) => (
        <div class={lineColor(line)}>{line || ' '}</div>
      )}</For>
      {truncated && (
        <div class="text-[var(--warning)] mt-1 font-medium">
          ⚠ {tt('chat.outputTruncated', { count: String(lines.length) })}
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
