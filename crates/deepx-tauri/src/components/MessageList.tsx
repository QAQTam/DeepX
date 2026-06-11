import { For, Show, createEffect, on } from "solid-js";
import MessageItem from "./MessageItem";
import type { Turn } from "../store/chat";
import { useI18n } from "../i18n";

interface MessageListProps { turns: Turn[]; isStreaming: () => boolean; onUndo: (turnId: string) => void; }

export default function MessageList(props: MessageListProps) {
  const { t } = useI18n();
  let listRef!: HTMLDivElement;
  let userScrolledUp = false;

  function isNearBottom(): boolean {
    if (!listRef) return true;
    const { scrollTop, scrollHeight, clientHeight } = listRef;
    return scrollHeight - scrollTop - clientHeight < 80;
  }

  function scrollBottom(force = false) {
    if (!listRef) return;
    if (force || !userScrolledUp || isNearBottom()) {
      listRef.scrollTo({ top: listRef.scrollHeight, behavior: force ? "auto" : "smooth" });
    }
  }

  // Track manual scroll
  function onScroll() {
    if (!listRef) return;
    userScrolledUp = !isNearBottom();
  }

  // Auto-scroll on new turns or streaming
  createEffect(on(() => props.turns.length, () => scrollBottom(true), { defer: true }));
  createEffect(on(() => props.turns[props.turns.length - 1]?.rounds.length, () => scrollBottom(false), { defer: true }));

  // Streaming: continuously scroll if user hasn't scrolled up
  createEffect(() => {
    if (props.isStreaming()) {
      const id = setInterval(() => scrollBottom(false), 200);
      return () => clearInterval(id);
    }
  });

  return (
    <div class="msg-list" ref={listRef} onScroll={onScroll}>
      <Show when={props.turns.length === 0} fallback={
        <For each={props.turns}>
          {(turn) => (
            <>
              <MessageItem role="user" text={turn.userText} turnId={turn.turnId} onUndo={props.onUndo} />
              <MessageItem role="assistant" rounds={turn.rounds} status={turn.status} />
            </>
          )}
        </For>
      }>
        <div class="msg-empty">
          <div class="msg-empty-icon">{">_"}</div>
          <div class="msg-empty-text">DeepX</div>
          <div class="msg-empty-hint">{t().chat.placeholder}</div>
        </div>
      </Show>
    </div>
  );
}
