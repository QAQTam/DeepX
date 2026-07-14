import { createEffect, createSignal, Show, onCleanup } from "solid-js";
import { useI18n } from "../i18n";
import type { ToolCallDef, ToolResultDef } from "../store/chat";
import AnsiUp from "ansi-to-html";
import { renderDiffHtml, isUnifiedDiff } from "../lib/diff";

function ansi() { return new AnsiUp({ escapeXML: true }); }

function fileName(argsJson: string): string {
  try { const a = JSON.parse(argsJson); const p = a.path || a.new_path || a.source || a.dest || ""; return String(p).replace(/\\/g, "/").split("/").pop() || ""; } catch (_) { return ""; }
}
function filePath(argsJson: string): string {
  try { const a = JSON.parse(argsJson); const p = a.path || a.new_path || a.source || a.dest || ""; return String(p).replace(/\\/g, "/"); } catch (_) { return ""; }
}
function execCmd(argsJson: string): string {
  try { const a = JSON.parse(argsJson); if (a.argv && Array.isArray(a.argv)) return a.argv.join(" "); return String(a.command || ""); } catch (_) { return ""; }
}
function webQuery(argsJson: string): string {
  try { const a = JSON.parse(argsJson); return String(a.query || a.url || ""); } catch (_) { return ""; }
}
function c7Lib(argsJson: string): string {
  try { const a = JSON.parse(argsJson); return String(a.library_name || a.name || a.library || ""); } catch (_) { return ""; }
}
function diffStats(output: string): { adds: number; dels: number } | null {
  if (!output) return null;
  try {
    const parsed = JSON.parse(output.trim());
    const d = parsed.diff || "";
    if (d && isUnifiedDiff(d)) {
      let a = 0, r = 0;
      for (const l of d.split("\n")) {
        if (l.startsWith("+++") || l.startsWith("---") || l.startsWith("@@")) continue;
        if (l.startsWith("+")) a++; else if (l.startsWith("-")) r++;
      }
      return a > 0 || r > 0 ? { adds: a, dels: r } : null;
    }
  } catch (_) {}
  if (!isUnifiedDiff(output)) return null;
  let a = 0, r = 0;
  for (const l of output.split("\n")) {
    if (l.startsWith("+++") || l.startsWith("---") || l.startsWith("@@")) continue;
    if (l.startsWith("+")) a++; else if (l.startsWith("-")) r++;
  }
  return a > 0 || r > 0 ? { adds: a, dels: r } : null;
}

/** Map tool name → i18n status key */
function statusKey(name: string): string {
  if (name.startsWith("read") || name === "read_file") return "reading";
  if (name.startsWith("edit") || name.startsWith("edit_file")) return "editing";
  if (name === "exec" || name === "exec_run") return "exec";
  if (name === "web_search") return "web";
  if (name === "web_fetch") return "webFetch";
  if (name === "web_context7_resolve") return "docResolve";
  if (name === "web_context7_query") return "docQuery";
  return name;
}

export default function ToolRow(props: { call: ToolCallDef; result?: ToolResultDef; streamingOutput?: string }) {
  const { t } = useI18n();
  const [open, setOpen] = createSignal(false);
  const [elapsed, setElapsed] = createSignal(0);

  const name = props.call.name;
  const hasResult = () => !!props.result;
  const isOk = () => !hasResult() || props.result!.success;
  const verb = (t().tool.status as Record<string, string>)[statusKey(name)] ?? name;

  const fileTools = ["read", "write", "edit", "edit_block", "list", "search", "diff", "delete"];
  const execTools = ["exec", "exec_run"];
  const webTools = ["web_search", "web_fetch"];
  const c7Tools = ["web_context7_resolve", "web_context7_query"];
  const expandable = fileTools.includes(name) || execTools.includes(name) || name === "sed";

  // Timer: tracks elapsed seconds, auto-stops when result arrives
  let timer: ReturnType<typeof setInterval> | null = null;
  createEffect(() => {
    if (hasResult()) {
      if (timer) { clearInterval(timer); timer = null; }
      return;
    }
    if (!timer) {
      timer = setInterval(() => setElapsed(s => s + 1), 1000);
    }
    onCleanup(() => { if (timer) { clearInterval(timer); timer = null; } });
  });

  if (name === "ask_user") return null;

  // ── Icon ──
  const icon = (): string => {
    if (fileTools.includes(name)) return "📄";
    if (execTools.includes(name)) return ">";
    if (webTools.includes(name)) return "@";
    if (c7Tools.includes(name)) return "📚";
    return "🔧";
  };

  // ── Detail text ──
  const detailText = (): string => {
    if (fileTools.includes(name)) {
      const fp = filePath(props.call.args_json);
      const stats = hasResult() ? diffStats(props.result!.output) : null;
      let s = fp || "…";
      if (stats) {
        const parts: string[] = [];
        if (stats.adds > 0) parts.push(`+${stats.adds}`);
        if (stats.dels > 0) parts.push(`−${stats.dels}`);
        if (parts.length) s += `  ${parts.join(" ")}`;
      }
      return s;
    }
    if (execTools.includes(name)) return execCmd(props.call.args_json);
    if (webTools.includes(name)) return webQuery(props.call.args_json);
    if (c7Tools.includes(name)) return c7Lib(props.call.args_json);
    return "";
  };

  // ── Status badge ──
  const badge = () => {
    if (!hasResult()) {
      return <span class="tc-badge running"><span class="tc-spinner" />{elapsed() > 0 ? `${elapsed()}s` : "…"}</span>;
    }
    if (isOk()) return <span class="tc-badge ok">✓</span>;
    return <span class="tc-badge err">✗</span>;
  };

  // ── Body ──
  const bodyHtml = (): string => {
    const raw = hasResult() ? props.result!.output : (props.streamingOutput || "");
    const clean = raw.replace(/^\[timeis:.*?\]\s*/gm, "")
      .replace(/^\[(OK|PARTIAL|DRY RUN)\].*\n/gm, "").trim();

    if (hasResult() && clean.startsWith("{")) {
      try {
        const parsed = JSON.parse(clean);
        if (parsed.diff && isUnifiedDiff(parsed.diff)) return renderDiffHtml(parsed.diff);
        if (parsed.output) return `<pre class="diff-plain">${ansi().toHtml(parsed.output)}</pre>`;
        if (parsed.content) return `<pre class="diff-plain">${ansi().toHtml(parsed.content)}</pre>`;
        return `<pre class="diff-plain">${ansi().toHtml(JSON.stringify(parsed, null, 2))}</pre>`;
      } catch (_) {}
    }
    if (isUnifiedDiff(clean)) return renderDiffHtml(clean);
    return `<pre class="diff-plain">${ansi().toHtml(clean)}</pre>`;
  };

  const statusCls = !hasResult() ? "running" : isOk() ? "ok" : "err";

  return (
    <div class={`tool-card ${statusCls} ${open() ? "expanded" : ""}`}>
      <div class="tc-bar" onClick={() => expandable && setOpen(o => !o)}>
        {expandable && <svg class={`tc-chevron ${open() ? "open" : ""}`} viewBox="0 0 12 12"><path d="M4 2l4 4-4 4" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>}
        <span class="tc-icon">{icon()}</span>
        <span class="tc-verb">{verb}</span>
        <span class="tc-detail">{detailText()}</span>
        {badge()}
      </div>
      <Show when={open() && expandable}>
        <div class="tc-body" innerHTML={bodyHtml() || "<span class=\"tc-spinner\" />"} />
      </Show>
    </div>
  );
}
