// ── SettingsDialog ──
// Provider → Endpoint → BaseUrl → Model hierarchy.
// Models from preset registry, not fetched.

import { createSignal, createEffect, onMount } from 'solid-js'
import { api, type ConfigData, type ProviderInfo, type EndpointInfo } from '../bridge/tauri'
import { tt, setLang } from '../i18n'
import { Button, Input, Select } from './shared'
import { useTheme, type Theme } from './shared/ThemeProvider'

interface SettingsDialogProps {
  onClose: () => void
}

function modelOptionsFor(endpoint: EndpointInfo | undefined, currentModel: string) {
  const list = (endpoint?.models?.length ?? 0) > 0 ? endpoint!.models : (endpoint?.default_model ? [endpoint.default_model] : [])
  const seen = new Set(list)
  const merged = list.slice()
  if (currentModel && !seen.has(currentModel) && currentModel.trim()) {
    merged.unshift(currentModel)
  }
  return merged
}

export function SettingsDialog(props: SettingsDialogProps) {
  const [apiKey, setApiKey] = createSignal('')
  const [baseUrl, setBaseUrl] = createSignal('')
  const [model, setModel] = createSignal('')
  const [contextLimit, setContextLimit] = createSignal(1000000)
  const [maxTokens, setMaxTokens] = createSignal(16384)
  const [providerId, setProviderId] = createSignal('')
  const [endpointId, setEndpointId] = createSignal('')
  const [reasoningEffort, setReasoningEffort] = createSignal('high')
  const [lang, setLangState] = createSignal('zh')
  const [context7Key, setContext7Key] = createSignal('')
  const [saving, setSaving] = createSignal(false)

  const [providers, setProviders] = createSignal<ProviderInfo[]>([])
  const { theme, setTheme } = useTheme()

  const currentProvider = (): ProviderInfo | undefined =>
    providers().find(p => p.id === providerId())

  const currentEndpoint = (): EndpointInfo | undefined =>
    currentProvider()?.endpoints.find(e => e.id === endpointId())

  const endpointsForProvider = (): EndpointInfo[] =>
    currentProvider()?.endpoints ?? []

  // ── Load config + providers ──
  onMount(() => {
    api.listProviders().then(list => {
      setProviders(list)
      if (list.length > 0) {
        const first = list[0]
        if (!providerId()) setProviderId(first.id)
        if (!endpointId()) setEndpointId(first.endpoints[0]?.id ?? '')
        if (!baseUrl()) setBaseUrl(first.endpoints[0]?.base_url ?? '')
        if (!model()) setModel(first.endpoints[0]?.default_model ?? '')
      }
    }).catch(e => console.error('listProviders failed:', e))

    api.loadConfig().then((cfg: ConfigData) => {
      if (cfg.api_key && cfg.api_key !== 'null') setApiKey(cfg.api_key)
      if (cfg.base_url) setBaseUrl(cfg.base_url)
      if (cfg.model) setModel(cfg.model)
      if (cfg.context_limit) setContextLimit(cfg.context_limit)
      if (cfg.max_tokens) setMaxTokens(cfg.max_tokens)
      if (cfg.provider_id) setProviderId(cfg.provider_id)
      if (cfg.endpoint) setEndpointId(cfg.endpoint)
      if (cfg.reasoning_effort) setReasoningEffort(cfg.reasoning_effort)
      if (cfg.lang) setLangState(cfg.lang)
      if (cfg.context7_api_key) setContext7Key(cfg.context7_api_key)
    }).catch(e => console.error('loadConfig failed:', e))
  })

  // ── Provider change → auto-fill endpoint, baseUrl, model ──
  createEffect(() => {
    const pid = providerId()
    const p = providers().find(p => p.id === pid)
    if (!p || p.endpoints.length === 0) return
    const ep = p.endpoints[0]
    if (endpointId() !== ep.id) setEndpointId(ep.id)
    setBaseUrl(ep.base_url)
    setModel(ep.default_model)
  })

  const save = async () => {
    setSaving(true)
    try {
      await api.saveConfig({
        apiKey: apiKey(), baseUrl: baseUrl(), model: model(),
        contextLimit: contextLimit(), maxTokens: maxTokens(),
        providerId: providerId(), endpoint: endpointId(), reasoningEffort: reasoningEffort(), lang: lang(), context7ApiKey: context7Key(),
      })
      api.reloadAgent().catch(e => console.error('reloadAgent failed:', e))
      setLang(lang())
      props.onClose()
    } catch(e) { console.error('saveConfig failed:', e) }
    finally { setSaving(false) }
  }

  return (
    <div class="absolute inset-0 bg-black/30 flex items-center justify-center z-50" role="dialog" aria-modal="true" aria-label={tt('settings.title')} onKeyDown={(e) => { if (e.key === 'Escape') props.onClose() }}>
      <div class="bg-[var(--bg-primary)] border border-[var(--border)] rounded-2xl p-6 max-w-md w-full mx-4 shadow-md transition-theme max-h-[90vh] overflow-y-auto">
        <div class="text-lg font-bold text-[var(--text-h)] mb-4">{tt('settings.title')}</div>

        <div class="space-y-4">
          {/* API Key */}
          <Input
            label={tt('settings.apiKey')}
            type="password"
            value={apiKey()}
            onInput={(e) => setApiKey(e.currentTarget.value)}
            placeholder="sk-..."
          />

          {/* Provider */}
          <Select
            label="Provider"
            options={providers().map(p => ({ value: p.id, label: p.display }))}
            value={providerId()}
            onChange={(e) => { setProviderId(e.currentTarget.value) }}
          />

          {/* Endpoint */}
          <Select
            label={tt('settings.endpoint')}
            options={endpointsForProvider().map(e => ({ value: e.id, label: e.display }))}
            value={endpointId()}
            onChange={(e) => {
              const epId = e.currentTarget.value
              setEndpointId(epId)
              const ep = currentProvider()?.endpoints.find(e => e.id === epId)
              if (ep) {
                setBaseUrl(ep.base_url)
                setModel(ep.default_model)
              }
            }}
          />

          {/* Base URL */}
          <Input
            label={tt('settings.baseUrl')}
            value={baseUrl()}
            onInput={(e) => setBaseUrl(e.currentTarget.value)}
          />

          {/* Model — preset from registry */}
          <Select
            label={tt('settings.model')}
            options={modelOptionsFor(currentEndpoint(), model()).map(m => ({ value: m, label: m }))}
            value={model()}
            onChange={(e) => setModel(e.currentTarget.value)}
          />

          {/* Context Limit */}
          <Input
            label={tt('settings.contextLimit')}
            type="number"
            value={String(contextLimit())}
            onInput={(e) => setContextLimit(Number(e.currentTarget.value))}
          />

          {/* Max Tokens */}
          <Input
            label={tt('settings.maxTokens')}
            type="number"
            value={String(maxTokens())}
            onInput={(e) => setMaxTokens(Number(e.currentTarget.value))}
          />

          {/* Thinking Effort */}
          <Select
            label={tt('settings.thinkingEffort')}
            options={[
              { value: 'no', label: 'No' },
              { value: 'low', label: 'Low' },
              { value: 'medium', label: 'Medium' },
              { value: 'high', label: 'High' },
              { value: 'xhigh', label: 'X-High' },
              { value: 'max', label: 'Max' },
            ]}
            value={reasoningEffort()}
            onChange={(e) => setReasoningEffort(e.currentTarget.value)}
          />

          {/* Context7 API Key */}
          <Input
            label={tt('settings.context7Key')}
            type="password"
            value={context7Key()}
            onInput={(e) => setContext7Key(e.currentTarget.value)}
            placeholder="c7-..."
          />

          {/* Language */}
          <Select
            label={tt('settings.language')}
            options={[
              { value: 'zh', label: '中文' },
              { value: 'en', label: 'English' },
            ]}
            value={lang()}
            onChange={(e) => setLangState(e.currentTarget.value)}
          />

          {/* Theme */}
          <Select
            label={tt('settings.theme')}
            options={[
              { value: 'system', label: tt('settings.themeSystem') },
              { value: 'dark', label: tt('settings.themeDark') },
              { value: 'light', label: tt('settings.themeLight') },
            ]}
            value={theme}
            onChange={(e) => setTheme(e.currentTarget.value as Theme)}
          />
        </div>

        <div class="flex gap-2 mt-6">
          <Button variant="secondary" onClick={props.onClose} class="flex-1">
            {tt('common.cancel')}
          </Button>
          <Button variant="primary" onClick={save} loading={saving()} class="flex-1">
            {saving() ? tt('settings.saving') : tt('common.save')}
          </Button>
        </div>
      </div>
    </div>
  )
}
