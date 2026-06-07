// ── MarkdownBody ──
// SolidJS: renders Markdown via marked + innerHTML with highlight.js

import { createMemo } from 'solid-js'
import { marked } from 'marked'
import hljs from 'highlight.js/lib/core'
import javascript from 'highlight.js/lib/languages/javascript'
import typescript from 'highlight.js/lib/languages/typescript'
import python from 'highlight.js/lib/languages/python'
import rust from 'highlight.js/lib/languages/rust'
import bash from 'highlight.js/lib/languages/bash'
import json from 'highlight.js/lib/languages/json'
import xml from 'highlight.js/lib/languages/xml'
import css from 'highlight.js/lib/languages/css'
import sql from 'highlight.js/lib/languages/sql'
import yaml from 'highlight.js/lib/languages/yaml'
import toml from 'highlight.js/lib/languages/ini'
import diff from 'highlight.js/lib/languages/diff'
import plaintext from 'highlight.js/lib/languages/plaintext'
import { tt } from '../../i18n'

hljs.registerLanguage('javascript', javascript)
hljs.registerLanguage('typescript', typescript)
hljs.registerLanguage('python', python)
hljs.registerLanguage('rust', rust)
hljs.registerLanguage('bash', bash)
hljs.registerLanguage('sh', bash)
hljs.registerLanguage('shell', bash)
hljs.registerLanguage('json', json)
hljs.registerLanguage('xml', xml)
hljs.registerLanguage('html', xml)
hljs.registerLanguage('css', css)
hljs.registerLanguage('sql', sql)
hljs.registerLanguage('yaml', yaml)
hljs.registerLanguage('toml', toml)
hljs.registerLanguage('ini', toml)
hljs.registerLanguage('diff', diff)
hljs.registerLanguage('plaintext', plaintext)
hljs.registerLanguage('text', plaintext)
hljs.registerLanguage('', plaintext)

interface MarkdownBodyProps {
  content: string
}

function sanitizeMarkdown(text: string): string {
  return text
    .replace(/<\|[\s\S]*?\|>/g, '')
    .replace(/<\|[^>]*>/g, '')
    .replace(/<\/?\w+(\s+[^>]*)?>/g, '')
    .replace(/```[\s\S]*?```|`[^`]*`/g, (m) => m.replace(/<\/?[^>]+>/g, ''))
}

const renderer: any = {
  code(...args: any[]) {
    const token = args[0]
    const codeText: string = typeof token === 'object' && 'text' in token ? token.text : String(args[0] || '')
    const lang: string = typeof token === 'object' && 'lang' in token ? token.lang : (args[1] || '')
    const highlighted = lang && hljs.getLanguage(lang)
      ? hljs.highlight(codeText, { language: lang }).value
      : codeText.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
    const escapedForAttr = codeText.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
    return `<div class="my-2 rounded-lg overflow-hidden border border-[var(--border)]">
      <div class="flex items-center justify-between px-3 py-1.5 bg-[var(--bg-tertiary)] text-xs text-[var(--muted)] font-medium">
        <span>${lang || 'code'}</span>
        <button class="copy-code-btn hover:text-[var(--text-h)] transition-colors"
          data-code="${escapedForAttr}">${tt('common.copy')}</button>
      </div>
      <pre class="!m-0 p-3 text-xs font-mono bg-[var(--bg-tertiary)] text-[var(--text)] overflow-x-auto"><code class="language-${lang}">${highlighted}</code></pre>
    </div>`
  },

  table(this: any, ...args: any[]) {
    const token = args[0]
    const text = typeof token === 'object' && typeof token.text === 'string' ? token.text
      : typeof token === 'object' && typeof token.raw === 'string' ? token.raw
      : String(args[0] || '')
    return `<div class="overflow-x-auto my-2"><table class="min-w-full text-xs border-collapse border border-[var(--border)]">${text}</table></div>`
  },
  tablecell(this: any, ...args: any[]) {
    const token = args[0]
    const flags = args[1]
    const text = typeof token === 'object' && typeof token.text === 'string' ? token.text
      : typeof args[0] === 'string' ? args[0] : ''
    const header = flags?.header || (typeof token === 'object' && token.flags?.header)
    const tag = header ? 'th' : 'td'
    const headerClass = header ? ' bg-[var(--bg-tertiary)] text-[var(--text-h)] font-medium text-left' : ''
    return `<${tag} class="border border-[var(--border)] px-2 py-1${headerClass}">${text}</${tag}>`
  },
  heading(this: any, ...args: any[]) {
    const token = args[0]
    const text = typeof token === 'object' && typeof token.text === 'string' ? token.text
      : typeof args[0] === 'string' ? args[0] : ''
    const depth = typeof token === 'object' && token.depth ? token.depth : (args[1] || 1)
    const sizes: Record<number, string> = { 1: 'text-lg font-bold mt-4 mb-2', 2: 'text-base font-bold mt-3 mb-1.5', 3: 'text-sm font-semibold mt-2 mb-1' }
    const cls = sizes[depth] || 'text-sm font-medium mt-2 mb-1'
    return `<h${depth} class="${cls} text-[var(--text-heading)]">${text}</h${depth}>`
  },
  blockquote(this: any, ...args: any[]) {
    const token = args[0]
    const text = typeof token === 'object' && typeof token.text === 'string' ? token.text
      : typeof token === 'object' && typeof token.raw === 'string' ? token.raw
      : typeof args[0] === 'string' ? args[0] : ''
    return `<blockquote class="border-l-3 border-[var(--accent)] pl-3 my-2 text-[var(--muted)] italic">${text}</blockquote>`
  },
}

marked.use({ gfm: true, renderer })

export function MarkdownBody(props: MarkdownBodyProps) {
  const html = createMemo(() => {
    const md = sanitizeMarkdown(props.content)
    const result = marked.parse(md)
    return typeof result === 'string' ? result : String(result)
  })
  return (
    <div
      class="prose prose-sm max-w-none text-[var(--text)] markdown-body"
      innerHTML={html()}
      onClick={(e) => {
        const btn = (e.target as Element).closest('.copy-code-btn')
        if (btn) {
          navigator.clipboard.writeText(btn.getAttribute('data-code') || '').catch(e => console.error('copy failed:', e))
        }
      }}
    />
  )
}
