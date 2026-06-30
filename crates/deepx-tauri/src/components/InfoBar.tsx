import { Show, createSignal, createEffect } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "../i18n";
import StockChart from "./StockChart";
import type { CodeDelta } from "../store/chat";

const FMT = (n: number) => n.toLocaleString();

export default function InfoBar(props: {
  model: string;
  seed: string;
  contextTokens: number;
  contextLimit: number;
  promptCacheHit: number;
  promptCacheMiss: number;
  isStreaming: boolean;
  error: string | null;
  onDismissError?: () => void;
  isCompacting: () => boolean;
  compactResult: () => string | null;
  onCompact?: () => void;
  codeDeltas: () => CodeDelta[];
}) {
  const { t } = useI18n();
  const seedShort = () => props.seed.substring(0, 8);

  const ctxPct = () =>
    props.contextLimit > 0 ? Math.round((props.contextTokens / props.contextLimit) * 100) : 0;

  const hitPct = () =>
    props.contextTokens > 0 ? Math.round((props.promptCacheHit / props.contextTokens) * 100) : 0;

  const hitLabel = () => {
    if (!props.contextTokens) return "—";
    return `${hitPct()}%`;
  };

  const [compactPct, setCompactPct] = createSignal(0);
  const [showChart, setShowChart] = createSignal(false);
  let compactTimer: ReturnType<typeof setInterval> | null = null;

  createEffect(() => {
    if (props.isCompacting()) {
      setCompactPct(0);
      compactTimer = setInterval(() => {
        setCompactPct((p) => {
          if (p >= 90) return 90;
          const step = Math.max(1, Math.floor((90 - p) * 0.08));
          return p + step;
        });
      }, 200);
    } else {
      if (compactTimer) { clearInterval(compactTimer); compactTimer = null; }
      if (props.compactResult()) {
        setCompactPct(100);
        setTimeout(() => setCompactPct(0), 2500);
      } else {
        setCompactPct(0);
      }
    }
  });

  async function handleCompact() {
    if (props.onCompact) { props.onCompact(); return; }
    try { await invoke("cmd_compact"); } catch (e) { console.error(e); }
  }

  return (
    <div>
    <div class="info-bar">
      <Show when={props.error}>
        <div class="info-error" onClick={props.onDismissError}>
          <span class="info-dot error" />
          <span>{props.error}</span>
        </div>
      </Show>
      <div class="info-item">
        <span class={`info-dot ${props.isStreaming ? "active" : props.error ? "error" : "idle"}`} />
        <span class="info-label">{t().infobar.model}</span>
        <span class="info-value">{props.model || "—"}</span>
      </div>
      <Show when={props.seed}>
        <div class="info-item">
          <span class="info-label">{t().infobar.session}</span>
          <span class="info-value mono">{seedShort()}</span>
        </div>
      </Show>
      <div class="info-item">
        <span class="info-label">{t().infobar.context}</span>
        <span class="info-value mono">{FMT(props.contextTokens)} / {FMT(props.contextLimit)}</span>
        <Show when={props.contextLimit > 0}>
          <span class="info-bar-pct" style={`--pct: ${ctxPct()}%`} />
        </Show>
      </div>
      <div class="info-item">
        <span class="info-label">{t().infobar.cacheHit}</span>
        <span class="info-value mono">{hitLabel()}</span>
        <Show when={props.contextTokens > 0}>
          <span class="info-bar-pct" style={`--pct: ${hitPct()}%`} />
        </Show>
      </div>
      <div class="info-item">
        <Show when={props.isCompacting() || compactPct() > 0}
          fallback={
            <button class="info-compact-btn" onClick={handleCompact} title="Compact history">
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                <polyline points="17 8 12 3 7 8" />
                <line x1="12" y1="3" x2="12" y2="15" />
              </svg>
            </button>
          }
        >
          <div class="compact-progress">
            <div class="compact-bar-bg">
              <div class="compact-bar-fill" style={`width: ${compactPct()}%`} />
            </div>
            <span class="compact-label">{props.isCompacting() ? `${compactPct()}%` : "✓"}</span>
          </div>
        </Show>
      </div>
      <div class="info-item">
        <button class="info-compact-btn" onClick={() => setShowChart(!showChart())} title="Stock chart">
          📈
        </button>
        <Show when={props.compactResult()}>
          <div class="compact-toast">{props.compactResult()}</div>
        </Show>
      </div>
    </div>
    <Show when={showChart()}>
      <div class="stock-chart-overlay" onClick={() => setShowChart(false)}>
        <div class="stock-chart-panel" onClick={(e) => e.stopPropagation()}>
          <div class="stock-chart-header">
            <span>📈 Code Stock</span>
            <button class="stock-chart-close" onClick={() => setShowChart(false)}>×</button>
          </div>
          <StockChart seed={props.seed} codeDeltas={props.codeDeltas} />
        </div>
      </div>
    </Show>
  </div>
  );
}
