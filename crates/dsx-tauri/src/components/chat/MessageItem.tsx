// ── MessageItem ──
// Renders a single user or assistant message.

import { For } from 'solid-js'
import type { Message, ToolCardEntry } from '../../types'
import { ReasoningBlock } from './ReasoningBlock'
import { MarkdownBody } from './MarkdownBody'
import { ToolCard } from './ToolCard'

interface MessageItemProps {
  msg: Message
}

export function MessageItem(props: MessageItemProps) {
  if (props.msg.role === 'user') {
    return (
      <div class="flex justify-end mb-4 anim-msg-in">
        <div class="max-w-[75%] bg-[var(--accent)] text-white rounded-2xl rounded-br-md px-4 py-2.5 text-sm leading-relaxed shadow-sm">
          <div class="whitespace-pre-wrap">{props.msg.content}</div>
        </div>
      </div>
    )
  }

  // Assistant message — may contain reasoning + content + tool cards
  const { reasoning, content, toolCards } = parseAssistant(props.msg.content, props.msg.reasoning, props.msg.tool_cards)

  return (
    <div class="mb-4 anim-msg-in">
      {/* Reasoning */}
      {reasoning && <ReasoningBlock content={reasoning} />}

      {/* Content */}
      {content && (
        <div class="max-w-[85%] bg-[var(--bg-secondary)] border border-[var(--border)] rounded-2xl rounded-bl-md px-4 py-3 text-sm leading-relaxed shadow-sm">
          <MarkdownBody content={content} />
        </div>
      )}

      {/* Tool Cards */}
      {toolCards && toolCards.length > 0 && (
        <div class="mt-2 space-y-2 max-w-[85%]">
          <For each={toolCards}>{(tc, i) => (
            <ToolCard ctx={{
              id: tc.id || `tc-${i()}`,
              name: tc.name,
              args: tc.args || '',
              body: tc.body,
              output: tc.output,
              success: tc.success,
            }} />
          )}</For>
        </div>
      )}
    </div>
  )
}

// ── Parser: extract reasoning from content ──
const REASONING_RE = /<reasoning>([\s\S]*?)<\/reasoning>/i

function parseAssistant(content: string, reasoning?: string, toolCards?: ToolCardEntry[]): {
  reasoning: string
  content: string
  toolCards?: ToolCardEntry[]
} {
  let thinking = ''
  let text = content
  const match = REASONING_RE.exec(content)
  if (match) {
    thinking = match[1].trim()
    text = content.replace(REASONING_RE, '').trim()
  }

  if (!thinking && reasoning) {
    thinking = reasoning
  }

  return { reasoning: thinking, content: text, toolCards }
}
