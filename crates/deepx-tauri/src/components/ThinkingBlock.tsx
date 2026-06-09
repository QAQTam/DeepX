import { createSignal, Show } from "solid-js";

export default function ThinkingBlock(props: { content: string }) {
  const [open, setOpen] = createSignal(true);
  return (
    <div class="think-block">
      <div class={`think-header ${open() ? "open" : ""}`} onClick={() => setOpen((o) => !o)}>
        <svg width="12" height="12" viewBox="0 0 12 12"><path d="M4 2l4 4-4 4" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>
        <span>Thinking</span>
      </div>
      <Show when={open()}>
        <div class="think-body">{props.content}</div>
      </Show>
    </div>
  );
}
