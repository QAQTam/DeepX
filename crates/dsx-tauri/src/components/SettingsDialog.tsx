import { useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { T } from '../i18n'

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
  const [lang, setLang] = useState('zh')
  const [saving, setSaving] = useState(false)

  const baseUrl = 'https://api.deepseek.com'

  useEffect(() => {
    invoke<any>('load_config').then(cfg => {
      if (cfg.api_key && cfg.api_key !== 'null') setApiKey(cfg.api_key)
      if (cfg.model) setModel(cfg.model)
      if (cfg.context_limit) setContextLimit(cfg.context_limit)
      if (cfg.max_tokens) setMaxTokens(cfg.max_tokens)
      if (cfg.effort) setEffort(cfg.effort)
      if (cfg.lang) setLang(cfg.lang)
      let cached = cfg.cached_models
      if (typeof cached === 'string') try { cached = JSON.parse(cached) } catch { /* ignore */ }
      if (Array.isArray(cached)) setModels(cached)
      else setModels([])
    }).catch(() => {})
  }, [])

  const fetchModels = () => {
    setFetching(true); setFetchError('')
    invoke<string[]>('fetch_models', { apiKey, baseUrl }).then(list => {
      setModels(list)
      setModel('')
      invoke('update_config', { field: 'cached_models', value: JSON.stringify(list.slice(0, 5)) }).catch(() => {})
    }).catch(e => { setFetchError(String(e)); setModels([]) }).finally(() => setFetching(false))
  }

  const save = () => {
    setSaving(true)
    invoke('save_config', { apiKey, baseUrl, model, contextLimit, maxTokens, effort, lang }).then(() => {
      invoke('reload_agent').catch(() => {})
      onClose()
    }).catch(() => {
      invoke('update_config', { field: 'api_key', value: apiKey })
      invoke('update_config', { field: 'base_url', value: baseUrl })
      invoke('update_config', { field: 'model', value: model })
      invoke('update_config', { field: 'context_limit', value: String(contextLimit) })
      invoke('update_config', { field: 'max_tokens', value: String(maxTokens) })
      invoke('update_config', { field: 'lang', value: lang })
      invoke('update_config', { field: 'effort', value: effort })
      invoke('reload_agent').catch(() => {})
      onClose()
    })
  }

  return (
    <div className="absolute inset-0 bg-black/30 flex items-center justify-center z-50">
      <div className="bg-[var(--bg-primary)] border border-[var(--border)] rounded-2xl p-6 max-w-lg w-full mx-4 shadow-md">
        <div className="text-sm font-bold text-[var(--text-h)] mb-4">{T.settings}</div>

        <div className="mb-3">
          <label className="block text-xs text-[var(--muted)] mb-1">API Key</label>
          <input type="password" value={apiKey} onChange={e => setApiKey(e.target.value)}
            placeholder="输入 DeepSeek API Key"
            className="w-full bg-[var(--bg-tertiary)] border border-[var(--border)] rounded-lg px-3 py-1.5 text-xs text-[var(--text-h)] font-mono outline-none focus:border-[var(--accent)]" />
        </div>

        <div className="mb-3">
          <label className="block text-xs text-[var(--muted)] mb-1">Base URL</label>
          <input type="text" value={baseUrl} readOnly
            className="w-full bg-[var(--bg-tertiary)] border border-[var(--border)] rounded-lg px-3 py-1.5 text-xs text-[var(--muted)] font-mono outline-none cursor-not-allowed" />
        </div>

        <div className="mb-3">
          <label className="block text-xs text-[var(--muted)] mb-1">Model</label>
          <div className="flex gap-2">
            {models.length > 0 ? (
              <select value={model} onChange={e => setModel(e.target.value)}
                className="flex-1 bg-[var(--bg-tertiary)] border border-[var(--border)] rounded-lg px-3 py-1.5 text-xs text-[var(--text-h)] font-mono outline-none focus:border-[var(--accent)]">
                {models.map(m => <option key={m} value={m}>{m}</option>)}
              </select>
            ) : (
              <input type="text" value={model} onChange={e => setModel(e.target.value)}
                className="flex-1 bg-[var(--bg-tertiary)] border border-[var(--border)] rounded-lg px-3 py-1.5 text-xs text-[var(--text-h)] font-mono outline-none focus:border-[var(--accent)]" />
            )}
            <button onClick={fetchModels} disabled={fetching || !apiKey}
              className="bg-[var(--bg-tertiary)] border border-[var(--border)] text-[var(--text-h)] rounded-lg px-3 py-1.5 text-xs hover:brightness-95 disabled:opacity-40 transition-colors shrink-0">
              {fetching ? '...' : '获取'}
            </button>
          </div>
          {fetchError && <div className="text-[10px] text-[var(--error)] mt-1">{fetchError}</div>}
        </div>

        <div className="grid grid-cols-2 gap-3 mb-3">
          <div>
            <label className="block text-xs text-[var(--muted)] mb-1">Context Limit</label>
            <input type="number" value={contextLimit} onChange={e => setContextLimit(Number(e.target.value))}
              className="w-full bg-[var(--bg-tertiary)] border border-[var(--border)] rounded-lg px-3 py-1.5 text-xs text-[var(--text-h)] font-mono outline-none focus:border-[var(--accent)]" />
          </div>
          <div>
            <label className="block text-xs text-[var(--muted)] mb-1">Max Tokens</label>
            <input type="number" value={maxTokens} onChange={e => setMaxTokens(Number(e.target.value))}
              className="w-full bg-[var(--bg-tertiary)] border border-[var(--border)] rounded-lg px-3 py-1.5 text-xs text-[var(--text-h)] font-mono outline-none focus:border-[var(--accent)]" />
          </div>
        </div>

        <div className="grid grid-cols-2 gap-3 mb-4">
          <div>
            <label className="block text-xs text-[var(--muted)] mb-1">思考强度</label>
            <div className="flex gap-1">
              {['high', 'max'].map(e => (
                <button key={e} onClick={() => setEffort(e)}
                  className={`flex-1 rounded-lg py-1.5 text-xs border transition-all ${effort === e ? 'bg-[var(--accent-light)] border-[var(--accent)] text-[var(--accent)] font-medium' : 'bg-[var(--bg-tertiary)] border-[var(--border)] text-[var(--text)]'}`}>
                  {e === 'high' ? '高' : '最高'}
                </button>
              ))}
            </div>
          </div>
          <div>
            <label className="block text-xs text-[var(--muted)] mb-1">{T.language}</label>
            <div className="flex gap-1">
              {['zh', 'en'].map(l => (
                <button key={l} onClick={() => setLang(l)}
                  className={`flex-1 rounded-lg py-1.5 text-xs border transition-all ${lang === l ? 'bg-[var(--accent-light)] border-[var(--accent)] text-[var(--accent)] font-medium' : 'bg-[var(--bg-tertiary)] border-[var(--border)] text-[var(--text)]'}`}>
                  {l === 'zh' ? '中文' : 'English'}
                </button>
              ))}
            </div>
          </div>
        </div>

        <div className="flex gap-3">
          <button onClick={onClose} className="flex-1 bg-[var(--bg-tertiary)] text-[var(--text-h)] rounded-lg py-2 text-sm hover:brightness-95">{T.cancel}</button>
          <button onClick={save} disabled={saving}
            className="flex-1 bg-[var(--accent)] text-white rounded-lg py-2 text-sm font-medium hover:brightness-110 disabled:opacity-40">
            {saving ? '保存中...' : T.save}
          </button>
        </div>
      </div>
    </div>
  )
}
