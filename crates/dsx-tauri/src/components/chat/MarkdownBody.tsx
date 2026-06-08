// ── MarkdownBody ──
// Renders Markdown via marked (zero deps) + shiki syntax highlighting.

import { createMemo, createSignal } from 'solid-js'
import { marked } from 'marked'

interface MarkdownBodyProps {
  content: string
}

// Module-level singleton: highlighter loads once
let _hl: any = null
const [_hlReady, setHlReady] = createSignal(false)

async function initHighlighter() {
  const { createHighlighter } = await import('shiki/bundle-full')
  _hl = await createHighlighter({
    themes: ['one-dark-pro'],
    langs: ['rust', 'typescript', 'javascript', 'bash', 'toml', 'json', 'python', 'c', 'cpp'],
  })
  setHlReady(true)
}
initHighlighter()

// Custom renderer: delegate fenced code blocks to shiki
const renderer = new marked.Renderer()
renderer.code = function ({ text, lang }: { text: string; lang?: string }): string {
  const escaped = escapeHtml(text)
  if (_hl && lang) {
    try {
      const highlighted = _hl.codeToHtml(text, { lang, theme: 'one-dark-pro' })
      return `<div class="code-block-wrapper relative group">
        <button class="code-block-copy absolute top-2 right-2 opacity-0 group-hover:opacity-100 text-xs px-2 py-1 rounded bg-[var(--bg-tertiary)] text-[var(--muted)] transition-opacity" data-code="${escaped.replace(/"/g, '&quot;')}" onclick="navigator.clipboard.writeText(this.dataset.code);this.textContent='Copied!';setTimeout(()=>this.textContent='Copy',2000)">Copy</button>
        ${highlighted}
      </div>`
    } catch {}
  }
  return `<pre><code>${escaped}</code></pre>`
}

function escapeHtml(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
}

export function MarkdownBody(props: MarkdownBodyProps) {
  const html = createMemo(() => {
    _hlReady()
    return marked.parse(props.content, { renderer, breaks: true }) as string
  })

  return (
    <div
      class="prose prose-sm max-w-none text-[var(--text)] markdown-body"
      innerHTML={html()}
    />
  )
}
