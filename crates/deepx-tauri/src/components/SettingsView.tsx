import { createSignal, createResource, For, Show, createEffect } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { useI18n, type Lang } from "../i18n";

interface Provider { id: string; display: string; endpoints: Endpoint[]; }
interface Endpoint { id: string; display: string; base_url: string; default_model: string; models: string[]; }

interface SettingsViewProps { lang: () => Lang; onLangChange: (l: Lang) => void; onClose: () => void; }

type SubPage = "index" | "api" | "limits" | "language";

export default function SettingsView(props: SettingsViewProps) {
  const { t } = useI18n();
  const [subpage, setSubpage] = createSignal<SubPage>("index");
  const [apiKey, setApiKey] = createSignal("");
  const [model, setModel] = createSignal("");
  const [baseUrl, setBaseUrl] = createSignal("");
  const [providerId, setProviderId] = createSignal("deepseek");
  const [endpointId, setEndpointId] = createSignal("openai");
  const [maxTokens, setMaxTokens] = createSignal(16384);
  const [contextLimit, setContextLimit] = createSignal(1000000);
  const [reasoningEffort, setReasoningEffort] = createSignal("high");
  const [saved, setSaved] = createSignal(false);

  const [configData] = createResource(async () => {
    try { const raw = await invoke<string>("cmd_load_config"); return JSON.parse(raw); }
    catch (e) { console.error(e); return null; }
  });

  createEffect(() => {
    const data = configData();
    if (!data) return;
    if (data.api_key) setApiKey(data.api_key === "****" ? "" : data.api_key);
    if (data.model) setModel(data.model);
    if (data.base_url) setBaseUrl(data.base_url);
    if (data.provider_id) setProviderId(data.provider_id);
    if (data.endpoint) setEndpointId(data.endpoint);
    if (data.max_tokens) setMaxTokens(data.max_tokens);
    if (data.context_limit) setContextLimit(data.context_limit);
    if (data.reasoning_effort) setReasoningEffort(data.reasoning_effort);
  });

  const providers = (): Provider[] => configData()?.providers ?? [];
  const currentEndpoints = (): Endpoint[] => { const p = providers().find((p: Provider) => p.id === providerId()); return p?.endpoints ?? []; };
  const currentModels = (): string[] => { const ep = currentEndpoints().find((e: Endpoint) => e.id === endpointId()); return ep?.models ?? [model()]; };

  function handleProviderChange(id: string) {
    setProviderId(id);
    const ep = providers().find((p: Provider) => p.id === id)?.endpoints[0];
    if (ep) { setEndpointId(ep.id); setBaseUrl(ep.base_url); setModel(ep.default_model); }
  }
  function handleEndpointChange(id: string) {
    setEndpointId(id);
    const ep = currentEndpoints().find((e: Endpoint) => e.id === id);
    if (ep) { setBaseUrl(ep.base_url); setModel(ep.default_model); }
  }

  async function save() {
    try {
      await invoke("cmd_save_config", { apiKey: apiKey(), model: model(), baseUrl: baseUrl(), providerId: providerId(), endpoint: endpointId(), maxTokens: maxTokens(), contextLimit: contextLimit(), reasoningEffort: reasoningEffort(), lang: props.lang() });
      setSaved(true);
      setTimeout(() => setSaved(false), 2500);
    } catch (e) { console.error(e); }
  }

  return (
    <div class="settings-view">
      {/* Header */}
      <div class="settings-header">
        <Show when={subpage() !== "index"}>
          <button class="settings-back-btn" onClick={() => setSubpage("index")}>
            <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="2"><path d="M10 2L4 8l6 6"/></svg>
          </button>
        </Show>
        <h2>{subpage() === "index" ? t().settings.title : subpageTitle(subpage())}</h2>
        <button class="settings-close-btn" onClick={props.onClose}>
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
        </button>
      </div>

      <Show when={!configData.loading} fallback={<div class="settings-loading">{t().chat.thinking}</div>}>
        <div class="settings-body">
          {/* Index page — category cards */}
          <Show when={subpage() === "index"}>
            <div class="settings-index">
              <button class="settings-card" onClick={() => setSubpage("api")}>
                <span class="settings-card-icon">A</span>
                <span class="settings-card-label">API</span>
                <span class="settings-card-desc">{providerId()} / {model()}</span>
                <svg class="settings-card-arrow" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M6 2l6 6-6 6"/></svg>
              </button>
              <button class="settings-card" onClick={() => setSubpage("limits")}>
                <span class="settings-card-icon">L</span>
                <span class="settings-card-label">Limits</span>
                <span class="settings-card-desc">{t().settings.maxTokens}: {maxTokens()} · ctx: {contextLimit()}</span>
                <svg class="settings-card-arrow" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M6 2l6 6-6 6"/></svg>
              </button>
              <button class="settings-card" onClick={() => setSubpage("language")}>
                <span class="settings-card-icon">i</span>
                <span class="settings-card-label">{t().settings.language}</span>
                <span class="settings-card-desc">{props.lang() === "en" ? "English" : "中文"}</span>
                <svg class="settings-card-arrow" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M6 2l6 6-6 6"/></svg>
              </button>
            </div>
          </Show>

          {/* API subpage */}
          <Show when={subpage() === "api"}>
            <div class="settings-panel">
              <div class="settings-field"><label>{t().settings.provider}</label><select value={providerId()} onChange={(e) => handleProviderChange(e.currentTarget.value)}><For each={providers()}>{(p: Provider) => <option value={p.id}>{p.display}</option>}</For></select></div>
              <div class="settings-field"><label>{t().settings.endpoint}</label><select value={endpointId()} onChange={(e) => handleEndpointChange(e.currentTarget.value)}><For each={currentEndpoints()}>{(ep: Endpoint) => <option value={ep.id}>{ep.display}</option>}</For></select></div>
              <div class="settings-field"><label>{t().settings.apiKey}</label><input type="password" value={apiKey()} onInput={(e) => setApiKey(e.currentTarget.value)} placeholder="sk-..." /><div class="hint">{t().settings.apiKeyHint}</div></div>
              <div class="settings-field"><label>{t().settings.model}</label><select value={model()} onChange={(e) => setModel(e.currentTarget.value)}><For each={currentModels()}>{(m: string) => <option value={m}>{m}</option>}</For></select></div>
              <div class="settings-field"><label>{t().settings.baseUrl}</label><input value={baseUrl()} onInput={(e) => setBaseUrl(e.currentTarget.value)} /></div>
              <div class="settings-actions"><button class="save" onClick={save}>{saved() ? t().settings.saved : t().settings.save}</button><button class="cancel" onClick={props.onClose}>{t().settings.cancel}</button></div>
            </div>
          </Show>

          {/* Limits subpage */}
          <Show when={subpage() === "limits"}>
            <div class="settings-panel">
              <div class="settings-field"><label>{t().settings.maxTokens}</label><input type="number" value={maxTokens()} onInput={(e) => setMaxTokens(parseInt(e.currentTarget.value) || 16384)} step={1024} /></div>
              <div class="settings-field"><label>{t().settings.contextLimit}</label><input type="number" value={contextLimit()} onInput={(e) => setContextLimit(parseInt(e.currentTarget.value) || 1000000)} step={100000} /></div>
              <div class="settings-field"><label>{t().settings.reasoningEffort}</label><select value={reasoningEffort()} onChange={(e) => setReasoningEffort(e.currentTarget.value)}><option value="high">high</option><option value="max">max</option></select></div>
              <div class="settings-actions"><button class="save" onClick={save}>{saved() ? t().settings.saved : t().settings.save}</button><button class="cancel" onClick={props.onClose}>{t().settings.cancel}</button></div>
            </div>
          </Show>

          {/* Language subpage */}
          <Show when={subpage() === "language"}>
            <div class="settings-panel">
              <div class="settings-field">
                <label>{t().settings.language}</label>
                <select value={props.lang()} onChange={(e) => props.onLangChange(e.currentTarget.value as Lang)}>
                  <option value="en">English</option>
                  <option value="zh">{"\u4e2d\u6587"}</option>
                </select>
              </div>
            </div>
          </Show>
        </div>
        <Show when={saved()}><div class="settings-toast">{t().settings.saved}</div></Show>
      </Show>
    </div>
  );
}

function subpageTitle(subpage: SubPage): string {
  switch (subpage) {
    case "api": return "API";
    case "limits": return "Limits";
    case "language": return "Language";
    default: return "";
  }
}
