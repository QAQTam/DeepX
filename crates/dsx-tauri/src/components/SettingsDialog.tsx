// ── SettingsDialog ──
// Full settings: API Key, model, context limit, max tokens, effort, language, theme.

import { useState, useEffect, type ChangeEvent } from 'react'
import { api, type ConfigData } from '../bridge/tauri'
import { tt, setLang } from '../i18n'
import { Button, Input, Select } from './shared'
import { useTheme, type Theme } from './shared/ThemeProvider'

interface SettingsDialogProps {
  onClose: () => void
}

export function SettingsDialog({ onClose }: SettingsDialogProps) {
  const [apiKey, setApiKey] = useState('')
  const [model, setModel] = useState('deepseek-v4-flash')
  const [models, setModels] = useState<string[]>([])
  const [fetching, setFetching] = useState(false)
  const [fetchError, setFetchError] = useState('')
  const [contextLimit, setContextLimit] = useState(1000000)
  const [maxTokens, setMaxTokens] = useState(16384)
  const [effort, setEffort] = useState('high')
  const [lang, setLangState] = useState('zh')
  const [saving, setSaving] = useState(false)
  const { theme, setTheme } = useTheme()

  useEffect(() => {
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
  }, [])

  const fetchModels = async () => {
    setFetching(true)
    setFetchError('')
    try {
      const list = await api.fetchModels(apiKey, 'https://api.deepseek.com')
      setModels(list)
      if (list.length > 0 && !list.includes(model)) setModel(list[0])
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
        apiKey, baseUrl: 'https://api.deepseek.com', model,
        contextLimit, maxTokens, effort, lang,
      })
      setLang(lang)
      await api.reloadAgent()
      onClose()
    } catch (e: any) {
      setFetchError(String(e))
      setSaving(false)
    }
  }

  const modelOptions = models.length > 0
    ? models.map(m => ({ value: m, label: m }))
    : [{ value: model, label: model }]

  return (
    <div className="absolute inset-0 bg-black/30 flex items-center justify-center z-50" onClick={e => { if (e.target === e.currentTarget) onClose() }}>
      <div className="bg-[var(--bg-primary)] border border-[var(--border)] rounded-2xl p-6 max-w-md w-full mx-4 shadow-lg max-h-[90vh] overflow-y-auto transition-theme">
        <div className="flex items-center justify-between mb-5">
          <h2 className="text-base font-bold text-[var(--text-h)]">{tt('settings.title')}</h2>
          <Button variant="ghost" size="sm" onClick={onClose}>✕</Button>
        </div>

        <div className="space-y-4">
          {/* API Key */}
          <Input
            type="password"
            label={tt('settings.apiKey')}
            placeholder={tt('settings.apiKeyPlaceholder')}
            hint={tt('settings.apiKeyHint')}
            value={apiKey}
            onChange={(e: ChangeEvent<HTMLInputElement>) => setApiKey(e.target.value)}
          />

          {/* Model */}
          <div className="flex flex-col gap-1">
            <Select
              label={tt('settings.model')}
              value={model}
              onChange={(e: ChangeEvent<HTMLSelectElement>) => setModel(e.target.value)}
              options={modelOptions}
            />
            <div className="flex items-center gap-2">
              <Button variant="secondary" size="sm" onClick={fetchModels} loading={fetching}>
                {fetching ? tt('settings.fetchingModels') : tt('settings.fetchModels')}
              </Button>
              {fetchError && <span className="text-xs text-[var(--error)]">{fetchError}</span>}
            </div>
          </div>

          {/* Context Limit */}
          <Select
            label={tt('settings.contextLimit')}
            value={String(contextLimit)}
            onChange={(e: ChangeEvent<HTMLSelectElement>) => setContextLimit(Number(e.target.value))}
            options={[
              { value: '131072', label: '128K' },
              { value: '262144', label: '256K' },
              { value: '524288', label: '512K' },
              { value: '1000000', label: '1M' },
            ]}
          />

          {/* Max Tokens */}
          <Select
            label={tt('settings.maxTokens')}
            value={String(maxTokens)}
            onChange={(e: ChangeEvent<HTMLSelectElement>) => setMaxTokens(Number(e.target.value))}
            options={[
              { value: '4096', label: '4K' },
              { value: '8192', label: '8K' },
              { value: '16384', label: '16K' },
              { value: '32768', label: '32K' },
            ]}
          />

          {/* Effort */}
          <Select
            label={tt('settings.effort')}
            value={effort}
            onChange={(e: ChangeEvent<HTMLSelectElement>) => setEffort(e.target.value)}
            options={[
              { value: 'high', label: tt('settings.effortHigh') },
              { value: 'max', label: tt('settings.effortMax') },
            ]}
          />

          {/* Language */}
          <Select
            label={tt('settings.lang')}
            value={lang}
            onChange={(e: ChangeEvent<HTMLSelectElement>) => setLangState(e.target.value)}
            options={[
              { value: 'zh', label: tt('settings.langZh') },
              { value: 'en', label: tt('settings.langEn') },
            ]}
          />

          {/* Theme */}
          <Select
            label={tt('settings.themeLabel')}
            value={theme}
            onChange={(e: ChangeEvent<HTMLSelectElement>) => setTheme(e.target.value as Theme)}
            options={[
              { value: 'system', label: tt('settings.themeSystem') },
              { value: 'light', label: tt('settings.themeLight') },
              { value: 'dark', label: tt('settings.themeDark') },
            ]}
          />
        </div>

        {/* Actions */}
        <div className="flex gap-3 mt-6">
          <Button variant="secondary" onClick={onClose} className="flex-1">
            {tt('common.cancel')}
          </Button>
          <Button variant="primary" onClick={save} loading={saving} className="flex-1">
            {saving ? tt('settings.saving') : tt('common.save')}
          </Button>
        </div>
      </div>
    </div>
  )
}
