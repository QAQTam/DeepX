import { createSignal, createResource, Show, For } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "../i18n";

// ── Types ──

interface DayStat {
  date: string;
  prompt_tokens: number;
  completion_tokens: number;
  cache_hit: number;
  cache_miss: number;
  calls: number;
}

interface TokenStats {
  daily: DayStat[];
  totals: {
    prompt_tokens: number;
    completion_tokens: number;
    calls: number;
    cache_hit_pct: number;
  };
}

// ── SVG Chart ──

const CHART_W = 640;
const CHART_H = 200;
const PAD_L = 52;
const PAD_R = 16;
const PAD_T = 8;
const PAD_B = 28;
const BAR_GAP = 2;

function fmtK(n: number): string {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + "M";
  if (n >= 1000) return (n / 1000).toFixed(1) + "k";
  return String(n);
}

function fmtPct(n: number): string {
  return n.toFixed(1) + "%";
}

export default function TokenChart(props: { refreshKey: number }) {
  const { t } = useI18n();
  const [range, setRange] = createSignal<7 | 30>(7);

  const [stats] = createResource(
    () => [range(), props.refreshKey] as const,
    async ([days]) => {
      try {
        const raw = await invoke<string>("cmd_get_token_stats", { days });
        return JSON.parse(raw) as TokenStats;
      } catch {
        return null;
      }
    },
  );

  const data = () => stats()?.daily ?? [];
  const totals = () => stats()?.totals;

  // Compute Y-axis max
  const yMax = () => {
    let max = 0;
    for (const d of data()) {
      const sum = d.prompt_tokens + d.completion_tokens;
      if (sum > max) max = sum;
    }
    // Round up to nice number
    if (max === 0) return 1000;
    const mag = Math.pow(10, Math.floor(Math.log10(max)));
    return Math.ceil(max / mag) * mag;
  };

  const plotW = () => CHART_W - PAD_L - PAD_R;
  const plotH = () => CHART_H - PAD_T - PAD_B;
  const barW = () => Math.max(2, Math.floor((plotW() - (data().length - 1) * BAR_GAP) / data().length / 2));

  // Y axis ticks
  const yTicks = () => {
    const max = yMax();
    const step = max / 4;
    return [0, step, step * 2, step * 3, max];
  };

  // X labels (abbreviated dates)
  const xLabel = (date: string) => {
    // "2025-01-15" → "1/15"
    const parts = date.split("-");
    if (parts.length === 3) return `${parseInt(parts[1])}/${parseInt(parts[2])}`;
    return date;
  };

  // Cache hit % line
  const hitPoints = () => {
    if (data().length === 0) return "";
    const n = data().length;
    const w = plotW();
    const h = plotH();
    const xStep = n > 1 ? w / (n - 1) : w;
    let d = "";
    for (let i = 0; i < n; i++) {
      const day = data()[i];
      const total = day.cache_hit + day.cache_miss;
      const pct = total > 0 ? (day.cache_hit / total) * 100 : 0;
      const x = PAD_L + (xStep * i);
      const y = PAD_T + h - (pct / 100) * h;
      d += `${i === 0 ? "M" : "L"}${x.toFixed(1)},${y.toFixed(1)} `;
    }
    return d;
  };

  return (
    <div class="token-chart">
      <div class="token-chart-header">
        <h3 class="token-chart-title">{t().tokenChart.title}</h3>
        <div class="token-chart-tabs">
          <button class={`token-chart-tab ${range() === 7 ? "active" : ""}`} onClick={() => setRange(7)}>
            {t().tokenChart.tabs.d7}
          </button>
          <button class={`token-chart-tab ${range() === 30 ? "active" : ""}`} onClick={() => setRange(30)}>
            {t().tokenChart.tabs.d30}
          </button>
        </div>
      </div>

      {/* ── Summary cards ── */}
      <Show when={totals()}>
        <div class="token-summary">
          <div class="token-summary-card">
            <span class="token-summary-val">{fmtK(totals()!.prompt_tokens + totals()!.completion_tokens)}</span>
            <span class="token-summary-label">{t().tokenChart.totalTokens}</span>
          </div>
          <div class="token-summary-card">
            <span class="token-summary-val">{totals()!.calls}</span>
            <span class="token-summary-label">{t().tokenChart.apiCalls}</span>
          </div>
          <div class="token-summary-card">
            <span class="token-summary-val">{fmtPct(totals()!.cache_hit_pct)}</span>
            <span class="token-summary-label">{t().tokenChart.cacheHit}</span>
          </div>
        </div>
      </Show>

      {/* ── SVG Chart ── */}
      <div class="token-chart-svg-wrap">
        <Show when={data().length === 0} fallback={
          <svg viewBox={`0 0 ${CHART_W} ${CHART_H}`} class="token-chart-svg">
            {/* Grid lines */}
            <For each={yTicks()}>
              {(tick) => {
                const y = PAD_T + plotH() - (tick / yMax()) * plotH();
                return (
                  <>
                    <line x1={PAD_L} y1={y} x2={CHART_W - PAD_R} y2={y} stroke="var(--border-card)" stroke-width="0.5" />
                    <text x={PAD_L - 6} y={y + 4} text-anchor="end" class="token-chart-axis" font-size="10">{fmtK(tick)}</text>
                  </>
                );
              }}
            </For>

            {/* Bars: prompt (solid) + completion (lighter) stacked */}
            <For each={data()}>
              {(day, i) => {
                const xStep = data().length > 1 ? plotW() / (data().length - 1) : plotW();
                const cx = PAD_L + xStep * i();
                const bw = barW();
                const promptH = (day.prompt_tokens / yMax()) * plotH();
                const compH = (day.completion_tokens / yMax()) * plotH();
                const promptY = PAD_T + plotH() - promptH - compH;
                const compY = PAD_T + plotH() - compH;
                return (
                  <>
                    <rect x={cx - bw} y={promptY} width={bw * 2} height={Math.max(0.5, promptH)} fill="var(--accent)" rx="1" />
                    <rect x={cx - bw} y={compY} width={bw * 2} height={Math.max(0.5, compH)} fill="var(--accent)" opacity="0.4" rx="1" />
                    {/* X label (every N days) */}
                    <Show when={data().length <= 10 || i() % Math.ceil(data().length / 10) === 0}>
                      <text x={cx} y={CHART_H - 4} text-anchor="middle" class="token-chart-axis" font-size="9">{xLabel(day.date)}</text>
                    </Show>
                  </>
                );
              }}
            </For>

            {/* Cache hit % line */}
            <Show when={data().length > 0}>
              <path d={hitPoints()} fill="none" stroke="var(--green)" stroke-width="1.5" opacity="0.8" />
              <For each={data()}>
                {(day, i) => {
                  const total = day.cache_hit + day.cache_miss;
                  if (total === 0) return null;
                  const pct = (day.cache_hit / total) * 100;
                  const xStep = data().length > 1 ? plotW() / (data().length - 1) : plotW();
                  const cx = PAD_L + xStep * i();
                  const cy = PAD_T + plotH() - (pct / 100) * plotH();
                  return <circle cx={cx} cy={cy} r="2.5" fill="var(--green)" />;
                }}
              </For>
            </Show>
          </svg>
        }>
          <div class="token-chart-empty">{t().tokenChart.empty}</div>
        </Show>
      </div>

      {/* Legend */}
      <div class="token-chart-legend">
        <span class="token-legend-item"><span class="token-legend-swatch accent" />{t().tokenChart.prompt}</span>
        <span class="token-legend-item"><span class="token-legend-swatch accent-light" />{t().tokenChart.completion}</span>
        <span class="token-legend-item"><span class="token-legend-swatch green" />{t().tokenChart.cacheHitPct}</span>
      </div>
    </div>
  );
}
