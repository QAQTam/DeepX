import { createSignal, Show, createEffect, on, onMount, onCleanup } from "solid-js";
import type { ToolCallDef, ToolResultDef } from "../store/chat";
import { useI18n } from "../i18n";
import AnsiUp from "ansi-to-html";
import { renderDiffHtml, isUnifiedDiff } from "../lib/diff";

function createAnsiRenderer() {
  return new AnsiUp({ escapeXML: true });
}

function renderOutput(text: string, ansi: AnsiUp): string {
  if (isUnifiedDiff(text)) return renderDiffHtml(text);
  return `<pre class="diff-plain">${ansi.toHtml(text)}</pre>`;
}

// ── Tool meta mapping ──
interface ToolMeta { icon: string; verb: string; }
function toolMeta(name: string): ToolMeta {
  const map: Record<string, ToolMeta> = {
    explore:        { icon: "🔍", verb: "explore" },
    read_file:      { icon: "📖", verb: "reading" },
    file_read:      { icon: "📖", verb: "reading" },
    write_file:     { icon: "✏️", verb: "writing" },
    file_write:     { icon: "✏️", verb: "writing" },
    edit_file:      { icon: "✂️", verb: "editing" },
    file_edit:      { icon: "✂️", verb: "editing" },
    edit_file_diff: { icon: "✂️", verb: "editing" },
    file_edit_diff: { icon: "✂️", verb: "editing" },
    delete_file:    { icon: "🗑", verb: "deleting" },
    file_delete:    { icon: "🗑", verb: "deleting" },
    file_move:      { icon: "📦", verb: "moving" },
    file_copy:      { icon: "📋", verb: "copying" },
    file_diff:      { icon: "=", verb: "diffing" },
    file_list:      { icon: "📂", verb: "listing" },
    list_dir:       { icon: "📂", verb: "listing" },
    file_search:    { icon: "🔎", verb: "searching" },
    grep:           { icon: "🔎", verb: "searching" },
    exec:           { icon: "⚡", verb: "exec" },
    exec_run:       { icon: "⚡", verb: "exec" },
    web_search:     { icon: "🌐", verb: "web" },
    web_fetch:      { icon: "🌐", verb: "webFetch" },
    git_add:        { icon: "📦", verb: "git" },
    git_commit:     { icon: "📦", verb: "git" },
    git_diff:       { icon: "📦", verb: "git" },
    git_log:        { icon: "📦", verb: "git" },
    git_show:       { icon: "📦", verb: "git" },
    git_status:     { icon: "📦", verb: "git" },
    task_create:    { icon: "📋", verb: "task" },
    task_update:    { icon: "📋", verb: "task" },
    task_delete:    { icon: "📋", verb: "task" },
    task_list:      { icon: "📋", verb: "task" },
    plan_create:    { icon: "📐", verb: "plan" },
    plan_update:    { icon: "📐", verb: "plan" },
    plan_list:      { icon: "📐", verb: "plan" },
    ask_user:       { icon: "❓", verb: "ask" },
    memory_read:    { icon: "🧠", verb: "memory" },
    memory_write:   { icon: "🧠", verb: "memory" },
    memory_clear:   { icon: "🧠", verb: "memory" },
    process_check:  { icon: "🔧", verb: "process" },
    process_kill:   { icon: "🔧", verb: "process" },
    process_wait:   { icon: "🔧", verb: "process" },
  };
  return map[name] ?? { icon: "•", verb: name };
}

export default function ToolCallCard(props: {
  call: ToolCallDef;
  result?: ToolResultDef;
  streamingOutput?: string;
}) {
  const { t } = useI18n();
  let bodyRef!: HTMLDivElement;
  const meta = toolMeta(props.call.name);
  const hasResult = !!props.result;
  const isOk = hasResult && props.result!.success;
  const [open, setOpen] = createSignal(false);
  const [elapsed, setElapsed] = createSignal(0);
  let timer: ReturnType<typeof setInterval> | null = null;

  onMount(() => {
    if (!hasResult) timer = setInterval(() => setElapsed((n) => n + 0.1), 100);
  });
  onCleanup(() => { if (timer) { clearInterval(timer); timer = null; } });
  createEffect(() => { if (hasResult && timer) { clearInterval(timer); timer = null; } });

  createEffect(on(() => [props.streamingOutput, props.result?.output], () => {
    if (bodyRef) bodyRef.scrollTop = bodyRef.scrollHeight;
  }));

  const fmtElapsed = () => {
    const s = elapsed();
    if (s < 1) return "0." + Math.floor(s * 10) + "s";
    if (s < 10) return s.toFixed(1) + "s";
    return Math.floor(s) + "s";
  };

  const verb = () => {
    const map = t().tool.status as Record<string, string>;
    return map[meta.verb] ?? meta.verb;
  };

  // State class: running (yellow) / ok (green) / err (red)
  const stateCls = () => !hasResult ? "capsule-running" : isOk ? "capsule-ok" : "capsule-err";

  const ansi = createAnsiRenderer();

  return (
    <div class={`tool-capsule ${stateCls()} ${open() ? "expanded" : ""}`}>
      <div class="capsule-bar" onClick={() => setOpen((o) => !o)}>
        <span class="capsule-icon">{meta.icon}</span>
        <span class="capsule-verb">{verb()}</span>
        <Show when={props.call.args_display}>
          <span class="capsule-args">{props.call.args_display}</span>
        </Show>
        <Show when={!hasResult}>
          <span class="capsule-timer">{fmtElapsed()}</span>
        </Show>
      </div>
      <Show when={open() && (hasResult || props.streamingOutput)}>
        <div class="capsule-body" ref={bodyRef} innerHTML={
          hasResult
            ? renderOutput(props.result!.output, ansi)
            : (props.streamingOutput ? renderOutput(props.streamingOutput, ansi) : "")
        } />
      </Show>
    </div>
  );
}
