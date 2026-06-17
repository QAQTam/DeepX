import { Show, createEffect } from "solid-js";
import { createVirtualizer } from "@tanstack/solid-virtual";
import MessageItem from "./MessageItem";
import type { Turn } from "../store/chat";
import { useI18n } from "../i18n";

interface MessageListProps {
  turns: Turn[];
  isStreaming: () => boolean;
  onUndo: (turnId: string) => void;
  hasMore: boolean;
  onLoadMore: () => void;
}

export default function MessageList(props: MessageListProps) {
  const { t } = useI18n();
  let listRef!: HTMLDivElement;
  const heightCache = new Map<string, number>();

  const virtualizer = createVirtualizer({
    get count() { return props.turns.length; },
    getScrollElement: () => listRef,
    estimateSize: (index: number) => {
      const tid = props.turns[index]?.turnId;
      return tid ? (heightCache.get(tid) ?? 600) : 600;
    },
    overscan: 50,
    anchorTo: "end",
    followOnAppend: true,
    getItemKey: (index: number) => props.turns[index]?.turnId ?? String(index),
  });

  // Scroll to end when turns are first loaded (restore / initial data)
  let prevLen = 0;
  createEffect(() => {
    const len = props.turns.length;
    if (len > 0 && prevLen === 0) {
      requestAnimationFrame(() => virtualizer.scrollToEnd());
    }
    prevLen = len;
  });

  // Auto-scroll to end on streaming
  createEffect(() => {
    if (props.isStreaming()) {
      const id = setInterval(() => {
        virtualizer.scrollToEnd({ behavior: "smooth" });
      }, 200);
      return () => clearInterval(id);
    }
  });

  // Jump-to-bottom button visibility
  const showJump = () => {
    const atEnd = virtualizer.isAtEnd();
    return !props.isStreaming() && props.turns.length > 0 && !atEnd;
  };

  return (
    <div class="msg-list-wrap">
      <Show when={props.hasMore}>
        <div class="load-more-bar">
          <button class="load-more-btn" onClick={props.onLoadMore}>
            {t().chat.loadEarlier}
          </button>
        </div>
      </Show>

      <div class="msg-list" ref={listRef}>
        <Show
          when={props.turns.length === 0}
          fallback={
            <div
              style={{
                height: `${virtualizer.getTotalSize()}px`,
                width: "100%",
                position: "relative",
              }}
            >
              {virtualizer.getVirtualItems().map((vItem) => {
                const turn = props.turns[vItem.index];
                return (
                  <div
                    ref={(el) => {
                      if (el) {
                        el.setAttribute("data-index", String(vItem.index));
                        virtualizer.measureElement(el);
                        const h = el.getBoundingClientRect().height;
                        if (h > 0) heightCache.set(turn.turnId, h);
                      }
                    }}
                    style={{
                      position: "absolute",
                      top: 0,
                      left: 0,
                      width: "100%",
                      transform: `translateY(${vItem.start}px)`,
                    }}
                  >
                    <MessageItem
                      role="user"
                      text={turn.userText}
                      turnId={turn.turnId}
                      onUndo={props.onUndo}
                    />
                    <MessageItem
                      role="assistant"
                      rounds={turn.rounds}
                      status={turn.status}
                    />
                  </div>
                );
              })}
            </div>
          }
        >
          <div class="msg-empty">
            <div class="msg-empty-icon">{">_"}</div>
            <div class="msg-empty-text">DeepX</div>
            <div class="msg-empty-hint">{t().chat.placeholder}</div>
          </div>
        </Show>
      </div>

      {/* Jump-to-bottom FAB */}
      <Show when={showJump()}>
        <button
          class="msg-jump-bottom"
          onClick={() => virtualizer.scrollToEnd({ behavior: "smooth" })}
        >
          {t().chat.jumpToLatest}
        </button>
      </Show>
    </div>
  );
}
