// ── SettingsDialog ──
// Full settings: API Key, model, context limit, max tokens, effort, language, theme.

import { createSignal, onMount } from 'solid-js'
import { api, type ConfigData } from '../bridge/tauri'
import { tt, setLang } from '../i18n'
import { Button, Input, Select } from './shared'
import { useTheme, type Theme } from './shared/ThemeProvider'

interface SettingsDialogProps {
  onClose: () => void
}

export function SettingsDialog(props: SettingsDialogProps) {
  const [apiKey, setApiKey] = createSignal('')
  const [baseUrl, setBaseUrl] = createSignal('https://api.deepseek.com')
  const [model, setModel] = createSignal('deepseek-v4-flash')
  const [models, setModels] = createSignal<string[]>([])
  const [fetching, setFetching] = createSignal(false)
  const [fetchError, setFetchError] = createSignal('')
  const [contextLimit, setContextLimit] = createSignal(1000000)
  const [maxTokens, setMaxTokens] = createSignal(16384)
  const [maxToolRounds, setMaxToolRounds] = createSignal(10)
  const [providerId, setProviderId] = createSignal('deepseek-openai')
  const [protocol, setProtocol] = createSignal('openai')
  const [isCustom, setIsCustom] = createSignal(false)
  const [reasoningEffort, setReasoningEffort] = createSignal('high')
  const [lang, setLangState] = createSignal('zh')
  const [context7Key, setContext7Key] = createSignal('')
  const [saving, setSaving] = createSignal(false)
  const { theme, setTheme } = useTheme()

  onMount(() => {
    api.loadConfig().then((cfg: ConfigData) => {
      if (cfg.api_key && cfg.api_key !== 'null') setApiKey(cfg.api_key)
      if (cfg.base_url) setBaseUrl(cfg.base_url)
      if (cfg.model) setModel(cfg.model)
      if (cfg.context_limit) setContextLimit(cfg.context_limit)
      if (cfg.max_tokens) setMaxTokens(cfg.max_tokens)
      if (cfg.max_tool_rounds) setMaxToolRounds(cfg.max_tool_rounds)
      if (cfg.provider_id) { setProviderId(cfg.provider_id); setIsCustom(cfg.provider_id === 'custom'); }
      if (cfg.protocol) setProtocol(cfg.protocol)
      if (cfg.reasoning_effort) setReasoningEffort(cfg.reasoning_effort)
      if (cfg.lang) setLangState(cfg.lang)
      if (cfg.context7_api_key) setContext7Key(cfg.context7_api_key)
    }).catch(() => {})
  })

  const fetchModels = async () => {
    setFetching(true)
    setFetchError('')
    try {
      const list = await api.fetchModels(apiKey(), baseUrl())
      setModels(list)
      if (list.length > 0 && !list.includes(model())) setModel(list[0])
    } catch (e: any) {
      setFetchError(String(e))
    } finally {
      setFetching(false)
    }
  }

  const save = async () => {
    setSaving(true)
    try {
      await api.saveConfig({
        apiKey: apiKey(), baseUrl: baseUrl(), model: model(),
        contextLimit: contextLimit(), maxTokens: maxTokens(), maxToolRounds: maxToolRounds(),
        providerId: providerId(), protocol: protocol(), reasoningEffort: reasoningEffort(), lang: lang(), context7ApiKey: context7Key(),
      })
      setLang(lang())
      props.onClose()
    } catch { /* ignore */ }
    finally { setSaving(false) }
  }

  return (
    <div class="absolute inset-0 bg-black/30 flex items-center justify-center z-50" role="dialog" aria-modal="true" aria-label={tt('settings.title')}>
      <div class="bg-[var(--bg-primary)] border border-[var(--border)] rounded-2xl p-6 max-w-md w-full mx-4 shadow-md transition-theme max-h-[90vh] overflow-y-auto">
        <div class="text-lg font-bold text-[var(--text-h)] mb-4">{tt('settings.title')}</div>

        <div class="space-y-4">
          {/* API Key */}
          <div>
            <Input
              label={tt('settings.apiKey')}
              type="password"
              value={apiKey()}
              onInput={(e) => setApiKey(e.currentTarget.value)}
              placeholder="sk-..."
            />
            <div class="flex items-center gap-2 mt-1">
              <Button variant="secondary" size="sm" onClick={fetchModels} loading={fetching()}>
                {tt('settings.fetchModels')}
              </Button>
              {fetchError() && <span class="text-xs text-[var(--error)]">{fetchError()}</span>}
            </div>
          </div>

          {/* Base URL */}
          <Input
            label={tt('settings.baseUrl')}
            value={baseUrl()}
            onInput={(e) => setBaseUrl(e.currentTarget.value)}
            placeholder="https://api.deepseek.com"
          />

          {/* Model */}
          <Select
            label={tt('settings.model')}
            options={models().map(m => ({ value: m, label: m }))}
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

          {/* Max Tool Rounds */}
          <Input
            label={tt('settings.maxToolRounds')}
            type="number"
            value={String(maxToolRounds())}
            onInput={(e) => setMaxToolRounds(Number(e.currentTarget.value))}
          />

          {/* Protocol */}
          <Select
            label={tt('settings.protocol')}
            options={[
              { value: 'openai', label: 'OpenAI / DeepSeek' },
              { value: 'anthropic', label: 'Anthropic' },
            ]}
            value={protocol()}
            onChange={(e) => setProtocol(e.currentTarget.value)}
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
            value={thinkingEffort()}
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
