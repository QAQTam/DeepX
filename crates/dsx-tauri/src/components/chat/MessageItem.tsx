// ── MessageItem ──
// Renders a single user or assistant message.

import type { Message, ToolCardEntry } from '../../types'
import { ReasoningBlock } from './ReasoningBlock'
import { MarkdownBody } from './MarkdownBody'
import { ToolCard } from './ToolCard'

interface MessageItemProps {
  msg: Message
}

export function MessageItem({ msg }: MessageItemProps) {
  if (msg.role === 'user') {
    return (
      <div className="flex justify-end mb-4 anim-msg-in">
        <div className="max-w-[75%] bg-[var(--accent)] text-white rounded-2xl rounded-br-md px-4 py-2.5 text-sm leading-relaxed shadow-sm">
          <div className="whitespace-pre-wrap">{msg.content}</div>
        </div>
      </div>
    )
  }

  // Assistant message — may contain reasoning + content + tool cards
  const { reasoning, content, toolCards } = parseAssistant(msg.content, msg.reasoning, msg.tool_cards)

  return (
    <div className="mb-4 anim-msg-in">
      {/* Reasoning */}
      {reasoning && <ReasoningBlock content={reasoning} />}

      {/* Content */}
      {content && (
        <div className="max-w-[85%] bg-[var(--bg-secondary)] border border-[var(--border)] rounded-2xl rounded-bl-md px-4 py-3 text-sm leading-relaxed shadow-sm">
          <MarkdownBody content={content} />
        </div>
      )}

      {/* Tool Cards */}
      {toolCards && toolCards.length > 0 && (
        <div className="mt-2 space-y-2 max-w-[85%]">
          {toolCards.map((tc, i) => (
            <ToolCard key={tc.id || i} ctx={{
              id: tc.id || `tc-${i}`,
              name: tc.name,
              args: tc.args || '',
              body: tc.body,
              output: tc.output,
              success: tc.success,
            }} />
          ))}
        </div>
      )}
    </div>
  )
}

// ── Parse assistant message ──
function parseAssistant(content: string, reasoning?: string, toolCards?: ToolCardEntry[]) {
  let thinking = ''
  let text = content

  // Extract <reasoning> blocks
  const re = /<reasoning>([\s\S]*?)<\/reasoning>/g
  const parts: string[] = []
  let m: RegExpExecArray | null
  while ((m = re.exec(content)) !== null) {
    parts.push(m[1])
  }
  if (parts.length > 0) {
    thinking = parts.join('\n')
    text = content.replace(re, '').trim()
  }

  // Also check explicit reasoning prop
  if (!thinking && reasoning) {
    thinking = reasoning
  }

  return { reasoning: thinking, content: text, toolCards }
}
