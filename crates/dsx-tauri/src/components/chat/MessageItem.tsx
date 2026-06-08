// ── MessageItem ──
// Renders a single user or assistant message.
// Assistant messages use ordered blocks[] to preserve stream sequence.

import { For, Match, Switch } from 'solid-js'
import type { Message, MessageBlock } from '../../types'
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
        <div class="max-w-[75%] bg-[var(--accent)] text-white rounded-2xl rounded-br-md px-4 py-2.5 text-[15px] leading-relaxed shadow-sm">
          <div class="whitespace-pre-wrap">{props.msg.content}</div>
        </div>
      </div>
      )
    }

  // System message (tool notices, warnings)
  if (props.msg.role === 'system') {
    return (
      <div class="flex justify-center mb-3 anim-fade-in">
        <div class="max-w-[80%] bg-[var(--warning-light)] border border-[var(--warning)]/30 rounded-lg px-3 py-2 text-xs text-[var(--warning)] font-mono">
          {props.msg.content}
        </div>
      </div>
    )
  }

  // Assistant message — render blocks in stream order
  return (
    <div class="mb-4 anim-msg-in">
      <For each={props.msg.blocks ?? []}>
        {(block) => (
          <Switch>
            <Match when={(block as MessageBlock).type === 'reasoning'}>
              <ReasoningBlock content={(block as any).content} />
            </Match>
            <Match when={(block as MessageBlock).type === 'text'}>
              <div class="max-w-[85%] bg-[var(--bg-secondary)] border border-[var(--border)] rounded-2xl rounded-bl-md px-4 py-3 text-[15px] leading-relaxed shadow-sm">
                <MarkdownBody content={(block as any).content} />
              </div>
            </Match>
            <Match when={(block as MessageBlock).type === 'tool'}>
              <ToolCard ctx={(block as any).card} />
            </Match>
          </Switch>
        )}
      </For>
    </div>
  )
}
