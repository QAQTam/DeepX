import { createSignal, Show, onCleanup, createEffect } from "solid-js";
import { invoke } from "@tauri-apps/api/core";

interface ContextStats {
  /** All values are token counts (CJK-aware heuristic), not character lengths */
  messages: number;
  chat_text: number;
  thinking: number;
  tool_calls: number;
  tool_results: number;
  tools_schema: number;
  system_prompt: number;
  thinking_blocks: number;
  tool_call_blocks: number;
}

const COLORS = [
  "#5a8a4a", // chat_text - green
  "#8b6baa", // thinking - purple
  "#d4783c", // tool_calls - orange
  "#c4553d", // tool_results - red
  "#6b8db5", // tools_schema - blue
  "#9b8d7a", // system_prompt - grey
];

const LABELS = ["Chat", "Thinking", "Tool Calls", "Tool Results", "Schema", "System"];

function buildPiePaths(stats: ContextStats): string {
  const values = [stats.chat_text, stats.thinking, stats.tool_calls, stats.tool_results, stats.tools_schema, stats.system_prompt];
  const total = values.reduce((a, b) => a + b, 0);
  if (total === 0) return "";

  let paths = "";
  let startAngle = 0;
  const cx = 60, cy = 60, r = 50;

  for (let i = 0; i < values.length; i++) {
    const slice = (values[i] / total) * 360;
    if (slice < 1) { startAngle += slice; continue; } // skip tiny slices
    const endAngle = startAngle + slice;
    const x1 = cx + r * Math.cos((startAngle - 90) * Math.PI / 180);
    const y1 = cy + r * Math.sin((startAngle - 90) * Math.PI / 180);
    const x2 = cx + r * Math.cos((endAngle - 90) * Math.PI / 180);
    const y2 = cy + r * Math.sin((endAngle - 90) * Math.PI / 180);
    const largeArc = slice > 180 ? 1 : 0;
    paths += `<path d="M${cx},${cy} L${x1},${y1} A${r},${r} 0 ${largeArc} 1 ${x2},${y2} Z" fill="${COLORS[i]}" stroke="var(--bg-primary)" stroke-width="1"/>`;
    startAngle = endAngle;
  }
  return paths;
}

export default function ContextPanel(props: { seed: string }) {
  const [stats, setStats] = createSignal<ContextStats | null>(null);
  const [open, setOpen] = createSignal(false);

  async function refresh() {
    if (!props.seed) return;
    try {
      const raw = await invoke<string>("cmd_get_context_stats", { seed: props.seed });
      setStats(JSON.parse(raw));
    } catch (e) { console.error("context_stats:", e); }
  }

  // Refresh every 5 seconds when open, and poll when closed (for badge updates)
  let timer: ReturnType<typeof setInterval>;
  createEffect(() => {
    clearInterval(timer);
    timer = setInterval(refresh, open() ? 3000 : 8000);
  });
  onCleanup(() => clearInterval(timer));
  refresh();

  const piePaths = () => stats() ? buildPiePaths(stats()!) : "";

  const total_tokens = () => {
    const s = stats();
    if (!s) return 0;
    // Values are already token counts (CJK-aware heuristic), no /4 needed
    return s.chat_text + s.thinking + s.tool_calls + s.tool_results + s.tools_schema + s.system_prompt;
  };

  const pct = (n: number) => {
    const s = stats();
    if (!s) return "0%";
    const total = s.chat_text + s.thinking + s.tool_calls + s.tool_results + s.tools_schema + s.system_prompt;
    return total > 0 ? Math.round(n * 100 / total) + "%" : "0%";
  };

  return (
    <div class="context-panel">
      <button class="context-panel-trigger" onClick={() => { setOpen(!open()); if (!open()) refresh(); }} title="Context breakdown">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M18 20V10M12 20V4M6 20v-6"/>
        </svg>
        <Show when={total_tokens() > 0}>
          <span class="context-token-badge">{total_tokens()}t</span>
        </Show>
      </button>
      <Show when={open()}>
        <div class="context-dropdown">
          <div class="context-dropdown-hd">
            <span>Context (~{total_tokens()} tokens)</span>
            <button class="context-close" onClick={() => setOpen(false)}>×</button>
          </div>
          <div class="context-dropdown-body">
            <Show when={stats() && total_tokens() > 0} fallback={<div class="context-empty">No context data yet. Send a message first.</div>}>
              <div class="context-pie-wrap">
                <svg viewBox="0 0 120 120" class="context-pie" innerHTML={piePaths()} />
              </div>
              <div class="context-legend">
                {stats() && LABELS.map((label, i) => {
                  const values = [stats()!.chat_text, stats()!.thinking, stats()!.tool_calls, stats()!.tool_results, stats()!.tools_schema, stats()!.system_prompt];
                  return (
                    <div class="context-legend-item">
                      <span class="context-legend-dot" style={`background: ${COLORS[i]}`} />
                      <span class="context-legend-label">{label}</span>
                      <span class="context-legend-pct">{pct(values[i])}</span>
                    </div>
                  );
                })}
              </div>
              <div class="context-detail">
                <span>Messages: {stats()?.messages}</span>
                <span>Thinking blocks: {stats()?.thinking_blocks}</span>
                <span>Tool calls: {stats()?.tool_call_blocks}</span>
              </div>
            </Show>
          </div>
        </div>
      </Show>
    </div>
  );
}
