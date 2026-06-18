import { createSignal, Show, createEffect, on } from "solid-js";
import type { ToolCallDef, ToolResultDef } from "../store/chat";
import { useI18n } from "../i18n";
import AnsiUp from "ansi-to-html";

const ansiUp = new AnsiUp();

const isUnifiedDiff = (text: string): boolean =>
  /^(--- (a\/|\/)|@@ -\d+)/m.test(text);

function esc(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

function renderDiff(text: string): string {
  const lines = text.split("\n");
  const rows: Array<{ line: string; oldLn: string; newLn: string; cls: string }> = [];
  let oldLn = 0, newLn = 0;
  let fileHdr = "";
  let summary = "";
  let started = false;

  for (const line of lines) {
    if (!started && !line.startsWith("--- ") && !line.startsWith("@@")) {
      if (line.trim()) summary += esc(line) + "\n";
      continue;
    }
    if (line.startsWith("--- ")) {
      fileHdr = esc(line.slice(4));
      started = true;
      continue;
    }
    if (line.startsWith("+++ ")) { continue; }
    if (!started) continue;

    const hunkMatch = line.match(/^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/);
    if (hunkMatch) {
      if (oldLn === 0 && newLn === 0) {
        oldLn = parseInt(hunkMatch[1]) - 1;
        newLn = parseInt(hunkMatch[3]) - 1;
      }
      continue;
    }

    if (line.startsWith("-")) {
      oldLn++;
      rows.push({ line: esc(line.slice(1)), oldLn: String(oldLn), newLn: "", cls: "diff-row-del" });
    } else if (line.startsWith("+")) {
      newLn++;
      rows.push({ line: esc(line.slice(1)), oldLn: "", newLn: String(newLn), cls: "diff-row-add" });
    } else {
      oldLn++; newLn++;
      rows.push({ line: esc(line), oldLn: String(oldLn), newLn: String(newLn), cls: "diff-row-ctx" });
    }
  }

  if (rows.length === 0) return "";

  let html = '<div class="diff-block">';
  if (summary) html += `<div class="diff-summary">${summary.trim()}</div>`;
  if (fileHdr) html += `<div class="diff-file-hdr">${fileHdr}</div>`;
  html += '<div class="diff-uni-wrap">';

  for (const row of rows) {
    html += `<div class="diff-uni-row ${row.cls}">`;
    html += `<span class="diff-uni-ln diff-uni-old">${row.oldLn}</span>`;
    html += `<span class="diff-uni-ln diff-uni-new">${row.newLn}</span>`;
    html += `<span class="diff-uni-body">${row.line}</span>`;
    html += '</div>';
  }

  html += '</div></div>';
  return html;
}

function renderOutput(text: string): string {
  if (isUnifiedDiff(text)) {
    return renderDiff(text);
  }
  // ANSI-to-HTML: preserve terminal colors in PTY output
  const html = ansiUp.toHtml(text);
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
  const stateClass = () =>
    hasResult
      ? props.result!.success ? "tool-success" : "tool-error"
      : "tool-running";

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
        </Show>
      </div>
      <Show when={open() && (hasResult || props.streamingOutput)} fallback={
        <Show when={!hasResult}>
          <div class="tool-card-body muted">{t().tool.running}...</div>
        </Show>
      }>
        <div class="tool-card-body" ref={bodyRef} innerHTML={
          (props.streamingOutput || "") +
          (hasResult && props.streamingOutput ? "\n\n" : "") +
          (hasResult ? renderOutput(props.result!.output) : "")
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
