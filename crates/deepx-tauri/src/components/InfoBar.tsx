import { Show } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "../i18n";
import ContextPanel from "./ContextPanel";
import type { MetricPoint } from "./StreamMetricsChart";

const FMT = (n: number) => n.toLocaleString();

export default function InfoBar(props: {
  model: string;
  seed: string;
  context_tokens: number;
  context_limit: number;
  prompt_cache_hit: number;
  prompt_cache_miss: number;
  metricHistory: MetricPoint[];
  isStreaming: boolean;
  error: string | null;
  onDismissError?: () => void;
  isCompacting: () => boolean;
  compactResult: () => number | null;
  onCompact?: () => void;
}) {
  const { t } = useI18n();
  const seedShort = () => props.seed.substring(0, 8);

  const ctxPct = () =>
    props.context_limit > 0 ? Math.round((props.context_tokens / props.context_limit) * 100) : 0;

  const hitPct = () =>
    props.context_tokens > 0 ? Math.round((props.prompt_cache_hit / props.context_tokens) * 100) : 0;

  const hitLabel = () => {
    if (!props.context_tokens) return "—";
    return `${hitPct()}%`;
  };

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
        <span class="info-value mono">{FMT(props.context_tokens)} / {FMT(props.context_limit)}</span>
        <Show when={props.context_limit > 0}>
          <span class="info-bar-pct" style={`--pct: ${ctxPct()}%`} />
        </Show>
      </div>
      <div class="info-item">
        <span class="info-label">{t().infobar.cacheHit}</span>
        <span class="info-value mono">{hitLabel()}</span>
        <Show when={props.context_tokens > 0}>
          <span class="info-bar-pct" style={`--pct: ${hitPct()}%`} />
        </Show>
      </div>
      <div class="info-item">
        <ContextPanel seed={props.seed} metricHistory={props.metricHistory} contextLimit={props.context_limit} />
        <Show
          when={!props.isCompacting() && !props.compactResult()}
          fallback={
            <div class="compact-status">
              <Show when={props.isCompacting()}>
                <span class="compact-label">Compacting…</span>
              </Show>
              <Show when={props.compactResult()}>
                <span class="compact-label">✓</span>
              </Show>
            </div>
          }
        >
          <button class="info-compact-btn" onClick={handleCompact} title="Compact history">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
              <polyline points="17 8 12 3 7 8" />
              <line x1="12" y1="3" x2="12" y2="15" />
            </svg>
          </button>
        </Show>
      </div>
      <Show when={props.compactResult() !== null}>
        <div class="compact-toast">{t().infobar.compacted.replace("{n}", String(props.compactResult()!))}</div>
      </Show>
    </div>
  </div>
  );
}
