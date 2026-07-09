import { createSignal, createResource, For, Show, createEffect } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { useI18n, type Lang } from "../i18n";

type ThemeMode = "system" | "light" | "dark" | "dark-gray";

interface Provider { id: string; display: string; endpoints: Endpoint[]; }
interface Endpoint { id: string; display: string; base_url: string; default_model: string; models: string[]; stateful?: boolean; }

interface SettingsViewProps {
  lang: () => Lang; onLangChange: (l: Lang) => void;
  theme: () => ThemeMode; onThemeChange: (t: ThemeMode) => void;
}

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
  const [complianceEnabled, setComplianceEnabled] = createSignal(true);
  const [saved, setSaved] = createSignal(false);
  const [showApiKey, setShowApiKey] = createSignal(false);
  const [showC7Key, setShowC7Key] = createSignal(false);

  // Subagent
  const [subModel, setSubModel] = createSignal("");
  const [subBaseUrl, setSubBaseUrl] = createSignal("");
  const [subApiKey, setSubApiKey] = createSignal("");
  const [subMaxTokens, setSubMaxTokens] = createSignal(4096);
  const [subTimeout, setSubTimeout] = createSignal(120);
  const [subTools, setSubTools] = createSignal<string[]>(["read_file", "search", "grep", "exec", "list_dir", "glob"]);
  const [showSubApiKey, setShowSubApiKey] = createSignal(false);

  const [configData] = createResource(async () => {
    try { const raw = await invoke<string>("cmd_load_config"); return JSON.parse(raw); }
    catch (e) { console.error(e); return null; }
  });

  const [allTools] = createResource(async () => {
    try { const raw = await invoke<string>("cmd_list_available_tools"); return JSON.parse(raw) as string[]; }
    catch (e) { console.error(e); return []; }
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
    if (data.context7_api_key) setContext7Key(data.context7_api_key === "****" ? "" : data.context7_api_key);
    if (data.compliance_enabled !== undefined) setComplianceEnabled(data.compliance_enabled);
    if (data.subagent) {
      if (data.subagent.model) setSubModel(data.subagent.model);
      if (data.subagent.base_url) setSubBaseUrl(data.subagent.base_url);
      if (data.subagent.api_key && data.subagent.api_key !== "****") setSubApiKey(data.subagent.api_key);
      if (data.subagent.max_tokens) setSubMaxTokens(data.subagent.max_tokens);
      if (data.subagent.timeout_secs) setSubTimeout(data.subagent.timeout_secs);
      if (data.subagent.default_tools?.length) setSubTools(data.subagent.default_tools);
    }
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
  function toggleTool(name: string) {
    setSubTools(prev => prev.includes(name) ? prev.filter(t => t !== name) : [...prev, name]);
  }

  async function save() {
    try {
      await invoke("cmd_save_config", {
        apiKey: apiKey(), model: model(), baseUrl: baseUrl(),
        providerId: providerId(), endpoint: endpointId(),
        maxTokens: maxTokens(), contextLimit: contextLimit(),
        reasoningEffort: reasoningEffort(), lang: props.lang(),
        context7ApiKey: context7Key(),
        subagentModel: subModel(), subagentBaseUrl: subBaseUrl(),
        subagentApiKey: subApiKey(), subagentMaxTokens: subMaxTokens(),
        subagentTimeoutSecs: subTimeout(), subagentDefaultTools: subTools(),
      });
      setSaved(true);
      setTimeout(() => setSaved(false), 2500);
    } catch (e) { console.error(e); }
  }

  const EyeIcon = (props: { show: boolean }) => (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
      {props.show
        ? <><path d="M17.94 17.94A10.07 10.07 0 0112 20c-7 0-11-8-11-8a18.45 18.45 0 015.06-5.94M9.9 4.24A9.12 9.12 0 0112 4c7 0 11 8 11 8a18.5 18.5 0 01-2.16 3.19m-6.72-1.07a3 3 0 11-4.24-4.24"/><line x1="1" y1="1" x2="23" y2="23"/></>
        : <><path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/></>
      }
    </svg>
  );

  const Loading = () => <div class="settings-loading">{t().chat.thinking}</div>;

  return (
    <div class="settings-page">
      <div class="settings-page-header">
        <h1>{t().settings.title}</h1>
        <button class="settings-save-btn" classList={{ saved: saved() }} onClick={save}>
          {saved() ? "✓ " + (t().settings.saved ?? "Saved") : t().settings.save}
        </button>
      </div>

      <Show when={!configData.loading} fallback={<Loading />}>
        <div class="settings-page-body">

          {/* ── Provider ── */}
          <section class="settings-card">
            <h2 class="settings-card-title">{t().settings.sectionProvider}</h2>
            <div class="settings-row">
              <label>{t().settings.provider}</label>
              <select value={providerId()} onChange={(e) => handleProviderChange(e.currentTarget.value)}>
                <For each={providers()}>{(p: Provider) => <option value={p.id}>{p.display}</option>}</For>
              </select>
            </div>
            <div class="settings-row">
              <label>{t().settings.endpoint}</label>
              <select value={endpointId()} onChange={(e) => handleEndpointChange(e.currentTarget.value)}>
                <For each={currentEndpoints()}>{(ep: Endpoint) => <option value={ep.id}>{ep.display}</option>}</For>
              </select>
            </div>
            <div class="settings-row">
              <label>{t().settings.baseUrl}</label>
              <input value={baseUrl()} onInput={(e) => setBaseUrl(e.currentTarget.value)} placeholder="https://api.deepseek.com/v1" />
            </div>
          </section>

          {/* ── Model ── */}
          <section class="settings-card">
            <h2 class="settings-card-title">{t().settings.sectionModel}</h2>
            <div class="settings-row">
              <label>{t().settings.model}</label>
              <div class="settings-input-group">
                <input list="model-suggestions" value={model()} onInput={(e) => setModel(e.currentTarget.value)} placeholder="e.g. deepseek-chat" />
                <datalist id="model-suggestions"><For each={currentModels()}>{(m: string) => <option value={m} />}</For></datalist>
                <div class="settings-hint">{t().settings.modelHint}</div>
              </div>
            </div>
            <div class="settings-row">
              <label>{t().settings.maxTokens}</label>
              <input type="number" value={maxTokens()} onInput={(e) => setMaxTokens(parseInt(e.currentTarget.value) || 16384)} step={1024} />
            </div>
            <div class="settings-row">
              <label>{t().settings.contextLimit}</label>
              <input type="number" value={contextLimit()} onInput={(e) => setContextLimit(parseInt(e.currentTarget.value) || 1000000)} step={100000} />
            </div>
            <div class="settings-row">
              <label>{t().settings.reasoningEffort}</label>
              <select value={reasoningEffort()} onChange={(e) => setReasoningEffort(e.currentTarget.value)}>
                <option value="high">high</option>
                <option value="max">max</option>
              </select>
            </div>
          </section>

          {/* ── API Keys ── */}
          <section class="settings-card">
            <h2 class="settings-card-title">{t().settings.sectionApi}</h2>
            <div class="settings-row">
              <label>{t().settings.apiKey}</label>
              <div class="settings-input-group">
                <div class="settings-secret-row">
                  <input type={showApiKey() ? "text" : "password"} value={apiKey()} onInput={(e) => setApiKey(e.currentTarget.value)} placeholder="sk-..." />
                  <button class="settings-eye-btn" onClick={() => setShowApiKey(v => !v)} title={showApiKey() ? t().settings.hide : t().settings.show}>
                    <EyeIcon show={showApiKey()} />
                  </button>
                </div>
                <div class="settings-hint">{t().settings.apiKeyHint}</div>
              </div>
            </div>
            <div class="settings-row">
              <label>{t().settings.context7Key}</label>
              <div class="settings-input-group">
                <div class="settings-secret-row">
                  <input type={showC7Key() ? "text" : "password"} value={context7Key()} onInput={(e) => setContext7Key(e.currentTarget.value)} placeholder="c7-..." />
                  <button class="settings-eye-btn" onClick={() => setShowC7Key(v => !v)} title={showC7Key() ? t().settings.hide : t().settings.show}>
                    <EyeIcon show={showC7Key()} />
                  </button>
                </div>
                <div class="settings-hint">{t().settings.context7KeyHint}</div>
              </div>
            </div>
          </section>

          {/* ── Subagent ── */}
          <section class="settings-card">
            <h2 class="settings-card-title">{t().settings.sectionSubagent}</h2>
            <p class="settings-card-desc">{t().settings.subagentDesc}</p>
            <div class="settings-row">
              <label>{t().settings.subagentModel}</label>
              <div class="settings-input-group">
                <input value={subModel()} onInput={(e) => setSubModel(e.currentTarget.value)} placeholder={t().settings.subagentInherit} />
                <div class="settings-hint">{t().settings.subagentModelHint}</div>
              </div>
            </div>
            <div class="settings-row">
              <label>{t().settings.subagentBaseUrl}</label>
              <input value={subBaseUrl()} onInput={(e) => setSubBaseUrl(e.currentTarget.value)} placeholder={t().settings.subagentInherit} />
            </div>
            <div class="settings-row">
              <label>{t().settings.subagentApiKey}</label>
              <div class="settings-input-group">
                <div class="settings-secret-row">
                  <input type={showSubApiKey() ? "text" : "password"} value={subApiKey()} onInput={(e) => setSubApiKey(e.currentTarget.value)} placeholder={t().settings.subagentInherit} />
                  <button class="settings-eye-btn" onClick={() => setShowSubApiKey(v => !v)} title={showSubApiKey() ? t().settings.hide : t().settings.show}>
                    <EyeIcon show={showSubApiKey()} />
                  </button>
                </div>
                <div class="settings-hint">{t().settings.subagentApiKeyHint}</div>
              </div>
            </div>
            <div class="settings-row">
              <label>{t().settings.subagentMaxTokens}</label>
              <input type="number" value={subMaxTokens()} onInput={(e) => setSubMaxTokens(parseInt(e.currentTarget.value) || 4096)} step={512} />
            </div>
            <div class="settings-row">
              <label>{t().settings.subagentTimeout}</label>
              <input type="number" value={subTimeout()} onInput={(e) => setSubTimeout(parseInt(e.currentTarget.value) || 120)} step={30} />
            </div>
            <div class="settings-row">
              <label>{t().settings.subagentTools}</label>
              <div class="settings-input-group">
                <div class="settings-checkbox-grid">
                  <For each={allTools() ?? []}>
                    {(name) => (
                      <label class={`settings-checkbox-item ${subTools().includes(name) ? "checked" : ""}`}>
                        <input type="checkbox" checked={subTools().includes(name)} onChange={() => toggleTool(name)} />
                        <span>{name}</span>
                      </label>
                    )}
                  </For>
                </div>
                <div class="settings-hint">{t().settings.subagentToolsHint}</div>
              </div>
            </div>
          </section>

          {/* ── Interface ── */}
          <section class="settings-card">
            <h2 class="settings-card-title">{t().settings.sectionInterface}</h2>
            <div class="settings-row">
              <label>{t().settings.theme}</label>
              <select value={props.theme()} onChange={(e) => props.onThemeChange(e.currentTarget.value as ThemeMode)}>
                <option value="system">{t().settings.themeSystem}</option>
                <option value="light">{t().settings.themeLight}</option>
                <option value="dark">{t().settings.themeDark}</option>
                <option value="dark-gray">{t().settings.themeDarkGray}</option>
              </select>
            </div>
            <div class="settings-row">
              <label>{t().settings.language}</label>
              <select value={props.lang()} onChange={(e) => props.onLangChange(e.currentTarget.value as Lang)}>
                <option value="en">English</option>
                <option value="zh">中文</option>
              </select>
            </div>
          </section>

          {/* ── Compliance ── */}
          <section class="settings-card">
            <h2 class="settings-card-title">{t().settings.sectionCompliance}</h2>
            <div class="settings-row">
              <label>{t().settings.complianceEnabled}</label>
              <div class="settings-input-group">
                <label class="settings-toggle">
                  <input type="checkbox" checked={complianceEnabled()} onChange={(e) => setComplianceEnabled(e.currentTarget.checked)} />
                  <span class="settings-toggle-track" />
                </label>
                <div class="settings-hint">{t().settings.complianceEnabledHint}</div>
              </div>
            </div>
          </section>

        </div>
      </Show>
    </div>
  );
}
