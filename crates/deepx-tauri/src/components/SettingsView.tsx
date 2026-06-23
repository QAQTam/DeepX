import { createSignal, createResource, For, Show, createEffect } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { useI18n, type Lang } from "../i18n";

type ThemeMode = "system" | "light" | "dark" | "dark-gray";

interface Provider { id: string; display: string; endpoints: Endpoint[]; }
interface Endpoint { id: string; display: string; base_url: string; default_model: string; models: string[]; }

interface SettingsViewProps { lang: () => Lang; onLangChange: (l: Lang) => void; onClose: () => void; theme: () => ThemeMode; onThemeChange: (t: ThemeMode) => void; }

export default function SettingsView(props: SettingsViewProps) {
  const { t } = useI18n();
  const [apiKey, setApiKey] = createSignal("");
  const [model, setModel] = createSignal("");
  const [baseUrl, setBaseUrl] = createSignal("");
  const [providerId, setProviderId] = createSignal("deepseek");
  const [endpointId, setEndpointId] = createSignal("openai");
  const [maxTokens, setMaxTokens] = createSignal(16384);
  const [contextLimit, setContextLimit] = createSignal(1000000);
  const [reasoningEffort, setReasoningEffort] = createSignal("high");
  const [context7Key, setContext7Key] = createSignal("");
  const [activeProfile, setActiveProfile] = createSignal("default");
  const [saved, setSaved] = createSignal(false);
  const [showApiKey, setShowApiKey] = createSignal(false);
  const [showC7Key, setShowC7Key] = createSignal(false);

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
    if (data.active_profile) setActiveProfile(data.active_profile);
    if (data.context7_api_key) setContext7Key(data.context7_api_key === "****" ? "" : data.context7_api_key);
  });

  const providers = (): Provider[] => configData()?.providers ?? [];
  const currentEndpoints = (): Endpoint[] => {
    const p = providers().find((p: Provider) => p.id === providerId());
    return p?.endpoints ?? [];
  };
  const currentModels = (): string[] => {
    const ep = currentEndpoints().find((e: Endpoint) => e.id === endpointId());
    return ep?.models ?? [];
  };

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
      await invoke("cmd_save_config", {
        apiKey: apiKey(), model: model(), baseUrl: baseUrl(),
        providerId: providerId(), endpoint: endpointId(),
        maxTokens: maxTokens(), contextLimit: contextLimit(),
        reasoningEffort: reasoningEffort(), lang: props.lang(),
        context7ApiKey: context7Key(),
      });
      setSaved(true);
      setTimeout(() => setSaved(false), 2500);
    } catch (e) { console.error(e); }
  }

  return (
    <div class="settings-overlay" onClick={(e) => { if (e.target === e.currentTarget) props.onClose(); }}>
      <div class="settings-float-card">
        {/* Header */}
        <div class="settings-float-header">
          <div class="settings-float-title">
            <h2>{t().settings.title}</h2>
            <span class="settings-float-subtitle">
              {t().settings.activeProfile}: {activeProfile()}
            </span>
          </div>
          <button class="settings-float-close" onClick={props.onClose} aria-label="Close">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>
            </svg>
          </button>
        </div>

        <Show when={!configData.loading} fallback={
          <div class="settings-loading">{t().chat.thinking}</div>
        }>
          <div class="settings-float-body">
            {/* ── Provider ── */}
            <div class="settings-section">
              <h3 class="settings-section-title">{t().settings.sectionProvider}</h3>
              <div class="settings-field">
                <label>{t().settings.provider}</label>
                <select value={providerId()} onChange={(e) => handleProviderChange(e.currentTarget.value)}>
                  <For each={providers()}>{(p: Provider) => <option value={p.id}>{p.display}</option>}</For>
                </select>
              </div>
              <div class="settings-field">
                <label>{t().settings.endpoint}</label>
                <select value={endpointId()} onChange={(e) => handleEndpointChange(e.currentTarget.value)}>
                  <For each={currentEndpoints()}>{(ep: Endpoint) => <option value={ep.id}>{ep.display}</option>}</For>
                </select>
              </div>
              <div class="settings-field">
                <label>{t().settings.baseUrl}</label>
                <input value={baseUrl()} onInput={(e) => setBaseUrl(e.currentTarget.value)} placeholder="https://api.example.com" />
              </div>
            </div>

            {/* ── Model ── */}
            <div class="settings-section">
              <h3 class="settings-section-title">{t().settings.sectionModel}</h3>
              <div class="settings-field">
                <label>{t().settings.model}</label>
                <input
                  list="model-suggestions"
                  value={model()}
                  onInput={(e) => setModel(e.currentTarget.value)}
                  placeholder="e.g. deepseek-chat"
                />
                <datalist id="model-suggestions">
                  <For each={currentModels()}>{(m: string) => <option value={m} />}</For>
                </datalist>
                <div class="hint">{t().settings.modelHint}</div>
              </div>
              <div class="settings-field">
                <label>{t().settings.maxTokens}</label>
                <input type="number" value={maxTokens()} onInput={(e) => setMaxTokens(parseInt(e.currentTarget.value) || 16384)} step={1024} />
              </div>
              <div class="settings-field">
                <label>{t().settings.contextLimit}</label>
                <input type="number" value={contextLimit()} onInput={(e) => setContextLimit(parseInt(e.currentTarget.value) || 1000000)} step={100000} />
              </div>
              <div class="settings-field">
                <label>{t().settings.reasoningEffort}</label>
                <select value={reasoningEffort()} onChange={(e) => setReasoningEffort(e.currentTarget.value)}>
                  <option value="high">high</option>
                  <option value="max">max</option>
                </select>
              </div>
            </div>

            {/* ── API Keys ── */}
            <div class="settings-section">
              <h3 class="settings-section-title">{t().settings.sectionApi}</h3>
              <div class="settings-field">
                <label>{t().settings.apiKey}</label>
                <div class="settings-secret-row">
                  <input
                    type={showApiKey() ? "text" : "password"}
                    value={apiKey()}
                    onInput={(e) => setApiKey(e.currentTarget.value)}
                    placeholder="sk-..."
                  />
                  <button
                    class="settings-toggle-btn"
                    onClick={() => setShowApiKey(v => !v)}
                    title={showApiKey() ? t().settings.hide : t().settings.show}
                  >
                    {showApiKey() ? (
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M17.94 17.94A10.07 10.07 0 0112 20c-7 0-11-8-11-8a18.45 18.45 0 015.06-5.94M9.9 4.24A9.12 9.12 0 0112 4c7 0 11 8 11 8a18.5 18.5 0 01-2.16 3.19m-6.72-1.07a3 3 0 11-4.24-4.24"/><line x1="1" y1="1" x2="23" y2="23"/></svg>
                    ) : (
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/></svg>
                    )}
                  </button>
                </div>
                <div class="hint">{t().settings.apiKeyHint}</div>
              </div>
              <div class="settings-field">
                <label>{t().settings.context7Key}</label>
                <div class="settings-secret-row">
                  <input
                    type={showC7Key() ? "text" : "password"}
                    value={context7Key()}
                    onInput={(e) => setContext7Key(e.currentTarget.value)}
                    placeholder="c7-..."
                  />
                  <button
                    class="settings-toggle-btn"
                    onClick={() => setShowC7Key(v => !v)}
                    title={showC7Key() ? t().settings.hide : t().settings.show}
                  >
                    {showC7Key() ? (
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M17.94 17.94A10.07 10.07 0 0112 20c-7 0-11-8-11-8a18.45 18.45 0 015.06-5.94M9.9 4.24A9.12 9.12 0 0112 4c7 0 11 8 11 8a18.5 18.5 0 01-2.16 3.19m-6.72-1.07a3 3 0 11-4.24-4.24"/><line x1="1" y1="1" x2="23" y2="23"/></svg>
                    ) : (
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/></svg>
                    )}
                  </button>
                </div>
                <div class="hint">{t().settings.context7KeyHint}</div>
              </div>
            </div>

            {/* ── Interface ── */}
            <div class="settings-section">
              <h3 class="settings-section-title">{t().settings.sectionInterface}</h3>
              <div class="settings-field">
                <label>{t().settings.theme}</label>
                <select value={props.theme()} onChange={(e) => props.onThemeChange(e.currentTarget.value as ThemeMode)}>
                  <option value="system">{t().settings.themeSystem}</option>
                  <option value="light">{t().settings.themeLight}</option>
                  <option value="dark">{t().settings.themeDark}</option>
                  <option value="dark-gray">{t().settings.themeDarkGray}</option>
                </select>
              </div>
              <div class="settings-field">
                <label>{t().settings.language}</label>
                <select value={props.lang()} onChange={(e) => props.onLangChange(e.currentTarget.value as Lang)}>
                  <option value="en">English</option>
                  <option value="zh">{"\u4e2d\u6587"}</option>
                </select>
              </div>
            </div>
          </div>

          {/* Footer */}
          <div class="settings-float-footer">
            <button class="settings-btn-save" onClick={save}>
              {saved() ? t().settings.saved : t().settings.save}
            </button>
            <button class="settings-btn-cancel" onClick={props.onClose}>
              {t().settings.cancel}
            </button>
          </div>
        </Show>
      </div>

      <Show when={saved()}>
        <div class="settings-toast">{t().settings.saved}</div>
      </Show>
    </div>
  );
}
