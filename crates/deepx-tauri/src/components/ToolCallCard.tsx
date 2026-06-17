import { createSignal, Show, createEffect, on } from "solid-js";
import type { ToolCallDef, ToolResultDef } from "../store/chat";

export default function ToolCallCard(props: {
  call: ToolCallDef;
  result?: ToolResultDef;
  streamingOutput?: string;
}) {
  const [open, setOpen] = createSignal(false);
  let bodyRef!: HTMLDivElement;
  const icon = toolIcon(props.call.name);
  const hasResult = !!props.result;
  const stateClass = () =>
    hasResult
      ? props.result!.success ? "tool-success" : "tool-error"
      : "tool-running";

  // Auto-expand when streaming output arrives
  createEffect(on(() => props.streamingOutput, (v) => {
    if (v) setOpen(true);
  }));

  // Auto-scroll terminal to bottom
  createEffect(on(() => props.streamingOutput, () => {
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
            {props.result!.success ? "OK" : "ERR"}
          </span>
        </Show>
        <Show when={!hasResult}>
          <span class="tool-card-status tool-running-text">Running...</span>
        </Show>
      </div>
      <Show when={open() && (hasResult || props.streamingOutput)}>
        <div class="tool-card-body" ref={bodyRef}>
          {props.streamingOutput || ""}
          {hasResult && props.streamingOutput ? "\n\n" : ""}
          {hasResult ? props.result!.output : ""}
        </div>
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
