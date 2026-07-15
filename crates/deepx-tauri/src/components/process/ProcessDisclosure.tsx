import { createEffect, createSignal, Show, type JSX } from "solid-js";

export function formatElapsed(elapsedMs?: number): string {
  if (elapsedMs === undefined) return "";
  if (elapsedMs < 1_000) return `${elapsedMs}ms`;
  const seconds = Math.round(elapsedMs / 100) / 10;
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  return `${minutes}m ${Math.round(seconds % 60)}s`;
}

export default function ProcessDisclosure(props: {
  status: "running" | "waiting" | "completed" | "failed" | "cancelled";
  elapsedMs?: number;
  tokensPerSec?: number;
  defaultOpen?: boolean;
  children?: JSX.Element;
}) {
  const [open, setOpen] = createSignal(
    props.defaultOpen ?? props.status !== "completed",
  );
  const panelId = `process-${Math.random().toString(36).slice(2)}`;

  createEffect(() => {
    if (props.defaultOpen !== undefined) return;
    if (props.status === "completed") setOpen(false);
  });

  const label = () => {
    const elapsed = formatElapsed(props.elapsedMs);
    const speed = props.status === "completed" && props.tokensPerSec
      ? ` · ${props.tokensPerSec} tok/s`
      : "";
    switch (props.status) {
      case "running": return elapsed ? `正在处理 ${elapsed}` : "正在处理";
      case "waiting": return "需要你的批准";
      case "completed": return elapsed ? `已处理 ${elapsed}${speed}` : "已处理";
      case "failed": return "处理失败";
      case "cancelled": return "已停止";
    }
  };

  const toggle = () => setOpen(!open());

  return (
    <section class="process-disclosure" data-process-disclosure>
      <button
        type="button"
        class="process-disclosure-trigger"
        aria-expanded={open()}
        aria-controls={panelId}
        onClick={toggle}
      >
        <span class={`process-status-dot is-${props.status}`} aria-hidden="true" />
        <span>{label()}</span>
        <span class="process-chevron" aria-hidden="true">›</span>
      </button>
      <Show when={open()}>
        <div id={panelId} class="process-disclosure-panel">
          {props.children}
        </div>
      </Show>
    </section>
  );
}
