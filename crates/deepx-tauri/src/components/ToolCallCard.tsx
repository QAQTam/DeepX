import { createSignal, Show, createEffect, on, onMount, onCleanup } from "solid-js";
import type { ToolCallDef, ToolResultDef } from "../store/chat";
import { useI18n } from "../i18n";
import AnsiUp from "ansi-to-html";
import { renderDiffHtml, isUnifiedDiff } from "../lib/diff";

// Per-component AnsiUp: avoids state pollution from split ANSI sequences across chunks
// A fresh instance is created for each ToolCallCard mount.
function createAnsiRenderer() {
  return new AnsiUp({ escapeXML: true });
}

function renderOutput(text: string, ansi: AnsiUp): string {
  if (isUnifiedDiff(text)) {
    return renderDiffHtml(text);
  }
  const html = ansi.toHtml(text);
  return `<pre class="diff-plain">${html}</pre>`;
}

export default function ToolCallCard(props: {
  call: ToolCallDef;
  result?: ToolResultDef;
  streamingOutput?: string;
}) {
  const { t } = useI18n();
  let bodyRef!: HTMLDivElement;
  const icon = toolIcon(props.call.name);
  const hasResult = !!props.result;
  // Default open for running tools so streaming output is immediately visible
  const [open, setOpen] = createSignal(!hasResult);
  const [elapsed, setElapsed] = createSignal(0);
  let timer: ReturnType<typeof setInterval> | null = null;

  // Track elapsed time while tool is running
  onMount(() => {
    if (!hasResult) {
      timer = setInterval(() => setElapsed((n) => n + 1), 1000);
    }
  });
  onCleanup(() => {
    if (timer) { clearInterval(timer); timer = null; }
  });
  // Stop timer when result arrives
  createEffect(() => {
    if (hasResult && timer) {
      clearInterval(timer);
      timer = null;
    }
  });
  const stateClass = () =>
    hasResult
      ? props.result!.success ? "tool-success" : "tool-error"
      : "tool-running";

  // Per-component ANSI renderer — avoids cross-chunk state pollution
  const ansi = createAnsiRenderer();

  // Auto-expand when streaming output arrives (redundant with default but safe)
  createEffect(on(() => props.streamingOutput, (v) => {
    if (v) setOpen(true);
  }));

  // Auto-expand when result arrives (tool completed)
  createEffect(on(() => props.result, (r) => {
    if (r) setOpen(true);
  }));

  // Auto-scroll to bottom on content change
  createEffect(on(() => [props.streamingOutput, props.result?.output], () => {
    if (bodyRef) bodyRef.scrollTop = bodyRef.scrollHeight;
  }));

  return (
    <div class={`tool-card ${stateClass()}`}>
      <div class="tool-card-header" onClick={() => setOpen((o) => !o)}>
        <span class="tool-card-icon">{icon}</span>
        <span class="tool-card-name">{props.call.name}</span>
        <span class="tool-card-args">{props.call.args_display}</span>
        <Show when={hasResult}>
          <span class={`tool-card-status ${props.result!.success ? "success" : "error"}`}>
            {props.result!.success ? t().tool.ok : t().tool.err}
          </span>
        </Show>
        <Show when={!hasResult}>
          <span class="tool-card-status tool-running-text">{t().tool.running}</span>
          <Show when={elapsed() > 0}>
            <span class="tool-card-elapsed">{elapsed()}s</span>
          </Show>
        </Show>
      </div>
      <Show when={open() && (hasResult || props.streamingOutput)} fallback={
        <Show when={!hasResult}>
          <div class="tool-card-body muted">{t().tool.running}...</div>
        </Show>
      }>
        <div class="tool-card-body" ref={bodyRef} innerHTML={
          hasResult
            ? renderOutput(props.result!.output, ansi)
            : (props.streamingOutput ? renderOutput(props.streamingOutput, ansi) : "")
        } />
      </Show>
    </div>
  );
}

function toolIcon(name: string): string {
  const icons: Record<string, string> = {
    read_file: "R", write_file: "W", edit_file: "E", delete_file: "D",
    exec: ">", explore: "S", search: "Z", glob: "G",
    web_search: "@", web_fetch: "@", list_dir: "L", diff: "=",
    task: "T", ask_user: "?",
  };
  return icons[name] ?? "*";
}

