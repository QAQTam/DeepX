import { createSignal, Show, onMount, onCleanup, createEffect, on } from "solid-js";
import { useI18n } from "../i18n";

export default function ThinkingBlock(props: { content: string; streaming?: boolean; elapsedMs?: number }) {
  const { t } = useI18n();
  const [open, setOpen] = createSignal(false);
  const [elapsed, setElapsed] = createSignal(0);
  let bodyRef!: HTMLDivElement;
  let timer: ReturnType<typeof setInterval> | null = null;
  let lastContentLen = 0;
  let lastContentTime = 0;
  let thinkingDone = false;

  // Start timer when content first appears during streaming
  createEffect(on(() => props.content, (c) => {
    if (c && props.streaming && !timer && !thinkingDone) {
      timer = setInterval(() => setElapsed((n) => n + 0.1), 100);
    }
  }, { defer: true }));

  // Detect when thinking stops (content stops growing for 800ms)
  createEffect(on(() => props.content, (c) => {
    const len = c.length;
    if (len !== lastContentLen) {
      lastContentLen = len;
      lastContentTime = Date.now();
    }
  }));

  // Periodic check: if content hasn't grown in 800ms, thinking is done
  onMount(() => {
    const check = setInterval(() => {
      if (timer && lastContentTime > 0 && Date.now() - lastContentTime > 800) {
        clearInterval(timer);
        timer = null;
        thinkingDone = true;
      }
    }, 500);
    onCleanup(() => clearInterval(check));
  });

  onCleanup(() => { if (timer) { clearInterval(timer); timer = null; } });

  // When streaming ends, stop timer and auto-collapse
  createEffect(() => {
    if (!props.streaming && timer) {
      clearInterval(timer);
      timer = null;
      thinkingDone = true;
    }
    if (!props.streaming) setOpen(false);
  });

  // Auto-scroll thinking body to bottom
  createEffect(on(() => props.content, () => {
    if (bodyRef && open()) bodyRef.scrollTop = bodyRef.scrollHeight;
  }, { defer: true }));

  const fmtElapsed = () => {
    // Prefer backend-computed elapsedMs for completed turns
    if (!props.streaming && props.elapsedMs != null) {
      const s = props.elapsedMs / 1000;
      if (s < 1) return "0." + Math.floor(s * 10) + "s";
      if (s < 10) return s.toFixed(1) + "s";
      return Math.floor(s) + "s";
    }
    const s = elapsed();
    if (s < 1) return "0." + Math.floor(s * 10) + "s";
    if (s < 10) return s.toFixed(1) + "s";
    return Math.floor(s) + "s";
  };

  const isActive = () => props.streaming && !thinkingDone;

  return (
    <div class="think-block">
      <div class={`think-header ${open() ? "open" : ""}`} onClick={() => setOpen((o) => !o)}>
        <svg class="think-chevron" width="12" height="12" viewBox="0 0 12 12">
          <path d="M4 2l4 4-4 4" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
        </svg>
        <Show when={isActive()} fallback={<span>{t().message.thinking} ({fmtElapsed()})</span>}>
          <span class={`think-label ${isActive() ? "shimmer" : ""}`}>{t().message.thinking}…</span>
          <span class="think-timer">{fmtElapsed()}</span>
        </Show>
      </div>
      <Show when={open()}>
        <div class="think-body" ref={bodyRef}>{props.content}</div>
      </Show>
    </div>
  );
}
