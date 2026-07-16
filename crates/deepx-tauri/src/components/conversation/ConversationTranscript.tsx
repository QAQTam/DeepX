import { createEffect, createSignal, Index, Show } from "solid-js";
import type { TurnViewModel } from "../../presentation/turnProjection";
import TurnGroup from "./TurnGroup";

export default function ConversationTranscript(props: {
  turns: TurnViewModel[];
  hasMore?: boolean;
  onLoadMore?: () => void | Promise<void>;
}) {
  let scroller!: HTMLDivElement;
  const [nearBottom, setNearBottom] = createSignal(true);
  let prevLen = 0;
  let prevLastId = "";

  const measure = () => {
    const remaining = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight;
    setNearBottom(remaining < 120);
  };

  const scrollToBottom = () => {
    if (typeof scroller?.scrollTo === "function") scroller.scrollTo({ top: scroller.scrollHeight });
    else if (scroller) scroller.scrollTop = scroller.scrollHeight;
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
    const len = props.turns.length;
    const lastId = props.turns[len - 1]?.turnId ?? "";
    const firstRender = prevLastId === "";
    const prepended = len > prevLen && lastId === prevLastId;
    const appended = len > prevLen && lastId !== prevLastId;
    const replaced = !firstRender && !prepended && lastId !== prevLastId;
    if (firstRender || replaced || (appended && nearBottom())) {
      queueMicrotask(scrollToBottom);
    }
    prevLen = len;
    prevLastId = lastId;
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
      <main class="conversation-transcript" aria-live="polite">
        <Index each={props.turns}>{(turn) => <TurnGroup turn={turn()} />}</Index>
      </main>
      <Show when={!nearBottom()}>
        <button
          type="button"
          class="jump-to-bottom"
          aria-label="跳到最新消息"
          onClick={() => scroller.scrollTo({ top: scroller.scrollHeight, behavior: "smooth" })}
        >↓</button>
      </Show>
    </div>
  );
}
