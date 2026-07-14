import { createEffect, createSignal, Index, Show } from "solid-js";
import type { TurnViewModel } from "../../presentation/turnProjection";
import TurnGroup from "./TurnGroup";

export default function ConversationTranscript(props: { turns: TurnViewModel[] }) {
  let scroller!: HTMLDivElement;
  const [nearBottom, setNearBottom] = createSignal(true);

  const measure = () => {
    const remaining = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight;
    setNearBottom(remaining < 120);
  };

  createEffect(() => {
    props.turns.length;
    if (nearBottom()) queueMicrotask(() => scroller?.scrollTo({ top: scroller.scrollHeight }));
  });

  return (
    <div class="conversation-scroll" ref={scroller} onScroll={measure}>
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
