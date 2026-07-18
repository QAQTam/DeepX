import { createEffect, createSignal, Index, onCleanup, onMount, Show } from "solid-js";
import type { TurnViewModel } from "../../presentation/turnProjection";
import TurnGroup from "./TurnGroup";

const BOTTOM_THRESHOLD = 120;

export default function ConversationTranscript(props: {
  turns: TurnViewModel[];
  hasMore?: boolean;
  onLoadMore?: () => void | Promise<void>;
}) {
  let scroller!: HTMLDivElement;
  let transcript!: HTMLElement;
  let scrollFrame: number | undefined;
  let resizeObserver: ResizeObserver | undefined;
  const [followTail, setFollowTail] = createSignal(true);

  const scrollToBottom = () => {
    if (typeof scroller?.scrollTo === "function") scroller.scrollTo({ top: scroller.scrollHeight });
    else if (scroller) scroller.scrollTop = scroller.scrollHeight;
  };

  const scheduleScrollToBottom = () => {
    if (!followTail() || scrollFrame !== undefined) return;
    scrollFrame = requestAnimationFrame(() => {
      scrollFrame = undefined;
      if (followTail()) scrollToBottom();
    });
  };

  const measure = () => {
    const remaining = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight;
    setFollowTail(remaining < BOTTOM_THRESHOLD);
  };

  async function loadOlder() {
    if (!props.onLoadMore) return;
    const distanceFromBottom = scroller.scrollHeight - scroller.scrollTop;
    await props.onLoadMore();
    queueMicrotask(() => {
      scroller.scrollTop = Math.max(0, scroller.scrollHeight - distanceFromBottom);
    });
  }

  createEffect(() => {
    props.turns;
    queueMicrotask(scheduleScrollToBottom);
  });

  onMount(() => {
    if (typeof ResizeObserver === "undefined") return;
    resizeObserver = new ResizeObserver(() => scheduleScrollToBottom());
    resizeObserver.observe(transcript);
  });

  onCleanup(() => {
    if (scrollFrame !== undefined) cancelAnimationFrame(scrollFrame);
    resizeObserver?.disconnect();
  });

  return (
    <div class="conversation-scroll" ref={scroller} onScroll={measure}>
      <Show when={props.hasMore && props.onLoadMore}>
        <button
          type="button"
          data-load-more
          class="load-more-turns"
          onClick={() => void loadOlder()}
        >加载更早消息</button>
      </Show>
      <main ref={transcript} class="conversation-transcript" aria-live="polite">
        <Index each={props.turns}>{(turn) => <TurnGroup turn={turn()} />}</Index>
      </main>
      <Show when={!followTail()}>
        <button
          type="button"
          class="jump-to-bottom"
          aria-label="跳到最新消息"
          onClick={() => {
            setFollowTail(true);
            scheduleScrollToBottom();
          }}
        >↓</button>
      </Show>
    </div>
  );
}
