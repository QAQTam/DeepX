import { Show } from "solid-js";
import { useI18n } from "../i18n";

const FMT = (n: number) => n.toLocaleString();

export default function InfoBar(props: {
  model: string;
  seed: string;
  contextTokens: number;
  contextLimit: number;
  totalTokens: number;
  promptCacheHit: number;
  promptCacheMiss: number;
}) {
  const { t } = useI18n();
  const seedShort = () => props.seed.substring(0, 8);

  const ctxPct = () =>
    props.contextLimit > 0 ? Math.round((props.totalTokens / props.contextLimit) * 100) : 0;

  const hitPct = () =>
    props.contextTokens > 0 ? Math.round((props.promptCacheHit / props.contextTokens) * 100) : 0;

  const hitLabel = () => {
    if (!props.contextTokens) return "—";
    return `${hitPct()}%`;
  };

  return (
    <div class="info-bar">
      <div class="info-item">
        <span class="info-label">模型</span>
        <span class="info-value">{props.model || "—"}</span>
      </div>
      <Show when={props.seed}>
        <div class="info-item">
          <span class="info-label">会话</span>
          <span class="info-value mono">{seedShort()}</span>
        </div>
      </Show>
      <div class="info-item">
        <span class="info-label">上下文</span>
        <span class="info-value mono">{FMT(props.totalTokens)} / {FMT(props.contextLimit)}</span>
        <Show when={props.contextLimit > 0}>
          <span class="info-bar-pct" style={`--pct: ${ctxPct()}%`} />
        </Show>
      </div>
      <div class="info-item">
        <span class="info-label">缓存</span>
        <span class="info-value mono">{hitLabel()}</span>
        <Show when={props.contextTokens > 0}>
          <span class="info-bar-pct" style={`--pct: ${hitPct()}%`} />
        </Show>
      </div>
    </div>
  );
}
