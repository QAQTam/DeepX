// ── MarkdownBody ──
// Markdown renderer with custom components and security filtering.

import { useMemo } from 'react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { tt } from '../../i18n'

interface MarkdownBodyProps {
  content: string
}

export function MarkdownBody({ content }: MarkdownBodyProps) {
  const safeContent = useMemo(() => sanitizeMarkdown(content), [content])

  return (
    <div className="prose prose-sm max-w-none text-[var(--text)] markdown-body">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          // Code blocks with language label + copy button
          code({ className, children, ...props }) {
            const match = /language-(\w+)/.exec(className || '')
            const isInline = !match
            if (isInline) return <code className="bg-[var(--bg-tertiary)] px-1 py-0.5 rounded text-xs font-mono text-[var(--accent)]" {...props}>{children}</code>
            return (
              <div className="my-2 rounded-lg overflow-hidden border border-[var(--border)]">
                <div className="flex items-center justify-between px-3 py-1.5 bg-[var(--bg-tertiary)] text-xs text-[var(--muted)] font-medium">
                  <span>{match[1]}</span>
                  <button
                    onClick={() => navigator.clipboard.writeText(String(children).replace(/\n$/, ''))}
                    className="hover:text-[var(--text-h)] transition-colors"
                    title={tt('common.copy')}
                  >
                    {tt('common.copy')}
                  </button>
                </div>
                <pre className="!m-0 p-3 text-xs font-mono bg-[var(--bg-tertiary)] text-[var(--text)] overflow-x-auto">
                  <code className={className} {...props}>{children}</code>
                </pre>
              </div>
            )
          },
          table({ children }) { return <div className="overflow-x-auto my-2"><table className="min-w-full text-xs border-collapse border border-[var(--border)]">{children}</table></div> },
          th({ children }) { return <th className="border border-[var(--border)] px-2 py-1 bg-[var(--bg-tertiary)] text-[var(--text-h)] font-medium text-left">{children}</th> },
          td({ children }) { return <td className="border border-[var(--border)] px-2 py-1">{children}</td> },
          h1({ children }) { return <h1 className="text-lg font-bold text-[var(--text-heading)] mt-4 mb-2">{children}</h1> },
          h2({ children }) { return <h2 className="text-base font-bold text-[var(--text-heading)] mt-3 mb-1.5">{children}</h2> },
          h3({ children }) { return <h3 className="text-sm font-semibold text-[var(--text-heading)] mt-2 mb-1">{children}</h3> },
          blockquote({ children }) { return <blockquote className="border-l-3 border-[var(--accent)] pl-3 my-2 text-[var(--muted)] italic">{children}</blockquote> },
          a({ href, children }) { return <a href={href} target="_blank" rel="noopener noreferrer" className="text-[var(--accent)] hover:underline">{children}</a> },
        }}
      >
        {safeContent}
      </ReactMarkdown>
    </div>
  )
}

// ── Security: remove DSML blocks and HTML/XML tags ──
function sanitizeMarkdown(text: string): string {
  return text
    .replace(/<\|[\s\S]*?\|>/g, '')          // DSML blocks
    .replace(/<\|[^>]*>/g, '')               // DSML inline
    .replace(/<\/?\w+(\s+[^>]*)?>/g, '')     // HTML/XML tags
    .replace(/```[\s\S]*?```|`[^`]*`/g, (m) => m.replace(/<\/?[^>]+>/g, ''))  // preserve code blocks
}
