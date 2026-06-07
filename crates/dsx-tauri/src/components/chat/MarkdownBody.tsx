// ── MarkdownBody ──
// SolidJS: renders Markdown via solid-markdown-wasm (WASM + syntect highlighting)

import { MarkdownRenderer } from 'solid-markdown-wasm'

interface MarkdownBodyProps {
  content: string
}

export function MarkdownBody(props: MarkdownBodyProps) {
  return (
    <div
      class="prose prose-sm max-w-none text-[var(--text)] markdown-body"
      onClick={(e) => {
        const btn = (e.target as Element).closest('.code-block-copy')
        if (btn) {
          const wrapper = btn.closest('.code-block-wrapper')
          const code = wrapper?.querySelector('pre code')?.textContent || ''
          navigator.clipboard.writeText(code).catch(e => console.error('copy failed:', e))
          btn.classList.add('copied')
          setTimeout(() => btn.classList.remove('copied'), 2000)
        }
      }}
    >
      <MarkdownRenderer
        markdown={props.content}
        theme="base16-ocean-dark"
        fallback={<div class="text-[var(--muted)] text-sm">Loading...</div>}
      />
    </div>
  )
}
