// ── ChatMessage (compatibility wrapper) ──
// Re-exports MessageItem. The old 496-line monolithic component has been
// split into: MessageItem, ReasoningBlock, MarkdownBody, ToolCard, ToolResult.

import type { Message } from '../types'
import { MessageItem } from './chat/MessageItem'

interface ChatMessageProps {
  msg: Message
}

export function ChatMessage({ msg }: ChatMessageProps) {
  return <MessageItem msg={msg} />
}
