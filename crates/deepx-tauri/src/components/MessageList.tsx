import { Show, createEffect, createSignal, onCleanup } from "solid-js";
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
  const [autoScroll, setAutoScroll] = createSignal(true);

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

  // Detect manual scroll away from bottom (debounced to avoid momentary misfires).
  // Hysteresis: larger threshold to LEAVE auto-scroll mode, smaller to RE-ENTER.
  // This prevents flickering when the virtualizer recalculates item heights
  // during streaming — the momentary scroll-position shift won't cross the
  // wider "leave" threshold, so autoScroll stays stable.
  let scrollTimer: ReturnType<typeof setTimeout> | null = null;
  const onScroll = () => {
    const el = listRef;
    if (!el) return;
    if (scrollTimer) return; // debounce: skip rapid scroll events
    scrollTimer = setTimeout(() => {
      scrollTimer = null;
      const dist = el.scrollHeight - el.scrollTop - el.clientHeight;
      if (autoScroll() && dist > 120) {
        setAutoScroll(false);
      } else if (!autoScroll() && dist < 30) {
        setAutoScroll(true);
      }
    }, 150);
  };

  // Scroll to end whenever turns grow (new turn appended, not just first load).
  // Double RAF ensures measureElement has updated the virtualizer's internal
  // measurements before we scroll, avoiding overshoot from estimated heights.
  let prevLen = 0;
  createEffect(() => {
    const len = props.turns.length;
    if (len > prevLen) {
      requestAnimationFrame(() => {
        requestAnimationFrame(() => {
          virtualizer.scrollToEnd();
        });
      });
    }
    prevLen = len;
  });

  // Reset auto-scroll when streaming starts (user hasn't scrolled up yet)
  createEffect(() => {
    if (props.isStreaming()) {
      setAutoScroll(true);
    }
  });

  // Auto-scroll to end during streaming — only if user hasn't scrolled up
  createEffect(() => {
    if (props.isStreaming() && autoScroll()) {
      const id = setInterval(() => {
        if (autoScroll()) {
          virtualizer.scrollToEnd({ behavior: "smooth" });
        }
      }, 200);
      onCleanup(() => clearInterval(id));
    }
  });

  // Jump-to-bottom button visibility
  const showJump = () => props.turns.length > 0 && !autoScroll();

  return (
    <div class="msg-list-wrap">
      <Show when={props.hasMore}>
        <div class="load-more-bar">
          <button class="load-more-btn" onClick={props.onLoadMore}>
            {t().chat.loadEarlier}
          </button>
        </div>
      </Show>

      <div class="msg-list" ref={listRef} onScroll={onScroll}>
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
          onClick={() => { virtualizer.scrollToEnd({ behavior: "smooth" }); setAutoScroll(true); }}
        >
          {t().chat.jumpToLatest}
        </button>
      </Show>
    </div>
  );
}
