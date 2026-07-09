import { createSignal, Show } from "solid-js";
import { useI18n } from "../i18n";
import type { ToolCallDef, ToolResultDef } from "../store/chat";
import AnsiUp from "ansi-to-html";
import { renderDiffHtml, isUnifiedDiff } from "../lib/diff";

function ansi() { return new AnsiUp({ escapeXML: true }); }

function fileName(argsJson: string): string {
  try { const a = JSON.parse(argsJson); const p = a.path || a.new_path || a.source || a.dest || ""; return String(p).replace(/\\/g, "/").split("/").pop() || ""; } catch (_) { return ""; }
}
function execCmd(argsJson: string): string {
  try { const a = JSON.parse(argsJson); return String(a.command || "").substring(0, 120); } catch (_) { return ""; }
}
function diffStats(output: string): string {
  if (!isUnifiedDiff(output)) return "";
  let a = 0, r = 0;
  for (const l of output.split("\n")) {
    if (l.startsWith("+++") || l.startsWith("---") || l.startsWith("@@")) continue;
    if (l.startsWith("+")) a++; else if (l.startsWith("-")) r++;
  }
  return a > 0 || r > 0 ? ` +${a} −${r}` : "";
}

/** Map tool name → i18n status key */
function statusKey(name: string): string {
  if (name.startsWith("file_read") || name === "read_file") return "reading";
  if (name.startsWith("file_write") || name === "write_file") return "writing";
  if (name.startsWith("file_edit") || name.startsWith("edit_file")) return "editing";
  if (name === "sed") return "sed";
  if (name.startsWith("file_delete") || name === "delete_file") return "deleting";
  if (name === "file_move") return "moving";
  if (name === "file_copy") return "copying";
  if (name === "file_diff") return "diffing";
  if (name.startsWith("file_list") || name === "list_dir") return "listing";
  if (name.startsWith("file_search") || name === "grep") return "searching";
  if (name === "exec" || name === "exec_run") return "exec";
  if (name === "web_search") return "web";
  if (name === "web_fetch") return "webFetch";
  if (name.startsWith("git_")) return "git";
  if (name.startsWith("task_")) return "task";
  if (name.startsWith("plan_")) return "plan";
  if (name === "ask_user") return "ask";
  if (name.startsWith("memory_")) return "memory";
  if (name.startsWith("process_")) return "process";
  if (name === "explore") return "explore";
  return name;
}

export default function ToolRow(props: { call: ToolCallDef; result?: ToolResultDef; streamingOutput?: string }) {
  const { t } = useI18n();
  const [open, setOpen] = createSignal(false);
  const name = props.call.name;
  const hasResult = !!props.result;
  const isOk = !hasResult || props.result!.success;

  const verb = () => (t().tool.status as Record<string, string>)[statusKey(name)] ?? name;

  const detail = (): string => {
    if (name.startsWith("file_") || name.startsWith("edit_file") || name === "grep" || name === "sed") {
      const f = fileName(props.call.args_json);
      return f + diffStats(props.result?.output || "");
    }
    if (name === "exec" || name === "exec_run") return execCmd(props.call.args_json);
    return "";
  };

  const expandable = name.startsWith("file_") || name.startsWith("edit_file") || name === "exec" || name === "exec_run" || name === "sed";
  const bodyHtml = (): string => {
    const raw = hasResult ? props.result!.output : (props.streamingOutput || "");
    // Strip timestamp prefix and [OK]/[PARTIAL]/[DRY RUN] header line
    const clean = raw.replace(/^\[timeis:.*?\]\s*/gm, "")
      .replace(/^\[(OK|PARTIAL|DRY RUN)\].*\n/m, "");
    // exec results are serialized ExecOutput JSON — extract just the output
    if (hasResult && (name === "exec" || name === "exec_run")) {
      try {
        const parsed = JSON.parse(clean.trim());
        const out = parsed.output || "";
        return `<pre class="diff-plain">${ansi().toHtml(out)}</pre>`;
      } catch (_) { /* fall through */ }
    }
    if (isUnifiedDiff(clean)) return renderDiffHtml(clean);
    return `<pre class="diff-plain">${ansi().toHtml(clean)}</pre>`;
  };

  return (
    <div class={`tool-row ${isOk ? "ok" : "err"} ${open() ? "open" : ""}`}>
      <div class="tool-row-bar" onClick={() => expandable && setOpen(o => !o)}>
        <svg class="tool-row-chevron" viewBox="0 0 12 12"><path d="M4 2l4 4-4 4" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>
        <span class="tool-row-verb">{verb()}</span>
        <span class="tool-row-detail">{detail()}</span>
        {!hasResult && <span class="tool-row-dot" />}
      </div>
      <Show when={open() && expandable && (hasResult || props.streamingOutput)}>
        <div class="tool-row-body" innerHTML={bodyHtml()} />
      </Show>
    </div>
  );
}
