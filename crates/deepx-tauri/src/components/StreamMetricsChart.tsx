import { createMemo } from "solid-js";

export interface MetricPoint {
  ts: number;            // Date.now()
  context_tokens: number;
  cache_hit: number;     // prompt_cache_hit_tokens
  cache_miss: number;    // prompt_cache_miss_tokens
}

interface Props {
  history: MetricPoint[];
  contextLimit: number;
  width?: number;
  height?: number;
}

const PAD_L = 42;
const PAD_R = 42;
const PAD_T = 8;
const PAD_B = 22;

export default function StreamMetricsChart(props: Props) {
  const W = props.width ?? 320;
  const H = props.height ?? 160;
  const plotW = W - PAD_L - PAD_R;
  const plotH = H - PAD_T - PAD_B;

  const points = () => props.history.length > 1 ? props.history : null;
  const startTs = () => points() ? points()![0].ts : 0;

  const maxTokens = () => {
    if (!points()) return props.contextLimit || 10000;
    let m = 0;
    for (const p of points()!) { if (p.context_tokens > m) m = p.context_tokens; }
    return Math.max(m, props.contextLimit || 10000, 1000);
  };

  const xScale = (ts: number) => {
    if (!points()) return PAD_L;
    const elapsed = (ts - startTs()) / 1000;
    const maxElapsed = Math.max((points()![points()!.length - 1].ts - startTs()) / 1000, 1);
    return PAD_L + (elapsed / maxElapsed) * plotW;
  };

  const yTokens = (t: number) => {
    const max = maxTokens();
    return PAD_T + plotH - (t / max) * plotH;
  };

  const yPct = (pct: number) => {
    return PAD_T + plotH - (pct / 100) * plotH;
  };

  const tokensPath = createMemo(() => {
    if (!points()) return "";
    return points()!.map((p, i) => {
      const cmd = i === 0 ? "M" : "L";
      return `${cmd}${xScale(p.ts).toFixed(1)},${yTokens(p.context_tokens).toFixed(1)}`;
    }).join(" ");
  });

  const cachePath = createMemo(() => {
    if (!points()) return "";
    return points()!.map((p, i) => {
      const pct = p.cache_hit + p.cache_miss;
      const rate = pct > 0 ? (p.cache_hit / pct) * 100 : 0;
      const cmd = i === 0 ? "M" : "L";
      return `${cmd}${xScale(p.ts).toFixed(1)},${yPct(rate).toFixed(1)}`;
    }).join(" ");
  });

  const cacheDots = createMemo(() => {
    if (!points()) return "";
    return points()!.map(p => {
      const pct = p.cache_hit + p.cache_miss;
      const rate = pct > 0 ? (p.cache_hit / pct) * 100 : 0;
      return `<circle cx="${xScale(p.ts).toFixed(1)}" cy="${yPct(rate).toFixed(1)}" r="3" fill="var(--green)" opacity="0.8"/>`;
    }).join("");
  });

  const tokenDots = createMemo(() => {
    if (!points()) return "";
    return points()!.map(p => {
      return `<circle cx="${xScale(p.ts).toFixed(1)}" cy="${yTokens(p.context_tokens).toFixed(1)}" r="2" fill="var(--accent)" opacity="0.7"/>`;
    }).join("");
  });

  // Y-axis labels (tokens)
  const yLabels = createMemo(() => {
    const max = maxTokens();
    const steps = 4;
    let html = "";
    for (let i = 0; i <= steps; i++) {
      const val = (max / steps) * i;
      const y = yTokens(val);
      html += `<text x="38" y="${y + 4}" text-anchor="end" class="context-chart-label">${fmt(val)}</text>`;
      if (i > 0) {
        html += `<line x1="${PAD_L}" y1="${y}" x2="${W - PAD_R}" y2="${y}" stroke="var(--border-divider)" stroke-width="0.5"/>`;
      }
    }
    return html;
  });

  // X-axis labels (seconds)
  const xLabels = createMemo(() => {
    const pts = points();
    if (!pts || pts.length < 2) return "";
    const maxSec = (pts[pts.length - 1].ts - startTs()) / 1000;
    const steps = Math.min(5, Math.max(2, Math.floor(maxSec / 5)));
    let html = "";
    for (let i = 0; i <= steps; i++) {
      const sec = (maxSec / steps) * i;
      const x = PAD_L + (sec / maxSec) * plotW;
      html += `<text x="${x}" y="${H - 4}" text-anchor="middle" class="context-chart-label">${sec.toFixed(0)}s</text>`;
    }
    return html;
  });

  return (
    <div class="stream-metrics-chart">
      <svg viewBox={`0 0 ${W} ${H}`} class="context-chart-svg">
        {/* Grid lines & Y labels */}
        <g innerHTML={yLabels()} />
        {/* X labels */}
        <g innerHTML={xLabels()} />
        {/* Cache hit % line (green) */}
        <path d={cachePath()} fill="none" stroke="var(--green)" stroke-width="1.8" opacity="0.8" />
        <g innerHTML={cacheDots()} />
        {/* Context tokens line (accent/orange) */}
        <path d={tokensPath()} fill="none" stroke="var(--accent)" stroke-width="1.8" />
        <g innerHTML={tokenDots()} />
      </svg>
      <div class="context-chart-legend">
        <span><span class="legend-swatch accent" />Context tokens</span>
        <span><span class="legend-swatch green" />Cache hit %</span>
      </div>
    </div>
  );
}

function fmt(n: number): string {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + "M";
  if (n >= 1000) return (n / 1000).toFixed(0) + "K";
  return String(Math.round(n));
}
