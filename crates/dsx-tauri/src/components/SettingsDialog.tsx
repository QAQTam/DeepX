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
  const [model, setModel] = createSignal('deepseek-v4-flash')
  const [models, setModels] = createSignal<string[]>([])
  const [fetching, setFetching] = createSignal(false)
  const [fetchError, setFetchError] = createSignal('')
  const [contextLimit, setContextLimit] = createSignal(1000000)
  const [maxTokens, setMaxTokens] = createSignal(16384)
  const [effort, setEffort] = createSignal('high')
  const [lang, setLangState] = createSignal('zh')
  const [saving, setSaving] = createSignal(false)
  const { theme, setTheme } = useTheme()

  onMount(() => {
    api.loadConfig().then((cfg: ConfigData) => {
      if (cfg.api_key && cfg.api_key !== 'null') setApiKey(cfg.api_key)
      if (cfg.model) setModel(cfg.model)
      if (cfg.context_limit) setContextLimit(cfg.context_limit)
      if (cfg.max_tokens) setMaxTokens(cfg.max_tokens)
      if (cfg.effort) setEffort(cfg.effort)
      if (cfg.lang) setLangState(cfg.lang)
      let cached = cfg.cached_models
      if (typeof cached === 'string') try { cached = JSON.parse(cached) } catch { /* ignore */ }
      if (Array.isArray(cached)) setModels(cached)
    }).catch(() => {})
  })

  const fetchModels = async () => {
    setFetching(true)
    setFetchError('')
    try {
      const list = await api.fetchModels(apiKey(), 'https://api.deepseek.com')
      setModels(list)
      if (list.length > 0 && !list.includes(model())) setModel(list[0])
      await api.updateConfig('cached_models', JSON.stringify(list.slice(0, 5)))
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
        apiKey: apiKey(), baseUrl: 'https://api.deepseek.com', model: model(),
        contextLimit: contextLimit(), maxTokens: maxTokens(), effort: effort(), lang: lang(),
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

          {/* Effort */}
          <Select
            label={tt('settings.effort')}
            options={[
              { value: 'low', label: tt('settings.effortLow') },
              { value: 'medium', label: tt('settings.effortMedium') },
              { value: 'high', label: tt('settings.effortHigh') },
            ]}
            value={effort()}
            onChange={(e) => setEffort(e.currentTarget.value)}
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
