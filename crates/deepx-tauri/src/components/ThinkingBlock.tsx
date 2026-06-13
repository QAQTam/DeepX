import { createSignal, Show, createEffect, on } from "solid-js";

export default function ThinkingBlock(props: { content: string; streaming?: boolean }) {
  const [open, setOpen] = createSignal(props.streaming ?? false);
  let bodyRef!: HTMLDivElement;

  // Auto-scroll thinking body to bottom while streaming
  createEffect(on(() => props.content, () => {
    if (bodyRef && open()) {
      bodyRef.scrollTop = bodyRef.scrollHeight;
    }
  }, { defer: true }));

  return (
    <div class="think-block">
      <div class={`think-header ${open() ? "open" : ""}`} onClick={() => setOpen((o) => !o)}>
        <svg width="12" height="12" viewBox="0 0 12 12"><path d="M4 2l4 4-4 4" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>
        <span>Thinking</span>
      </div>
      <Show when={open()}>
        <div class="think-body" ref={bodyRef}>{props.content}</div>
      </Show>
    </div>
  );
}
