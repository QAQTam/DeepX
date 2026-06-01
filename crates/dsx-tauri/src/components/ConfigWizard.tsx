import { useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { T } from '../i18n'

interface ConfigWizardProps {
  onDone: () => void
}

export function ConfigWizard({ onDone }: ConfigWizardProps) {
  const [step, setStep] = useState(1)
  const [apiKey, setApiKey] = useState('')
  const [model, setModel] = useState('deepseek-v4-flash')
  const [contextLimit, setContextLimit] = useState(1000000)
  const [saving, setSaving] = useState(false)
  const [saveError, setSaveError] = useState('')

  const finish = () => {
    setSaving(true)
    setSaveError('')
    invoke('save_config', {
      apiKey, baseUrl: 'https://api.deepseek.com', model, contextLimit,
      maxTokens: 16384, effort: 'high', lang: 'zh',
    }).then(onDone).catch((e: any) => { setSaving(false); setSaveError(String(e)) })
  }

  return (
    <div className="h-full flex items-center justify-center bg-[var(--bg-primary)]">
      <div className="bg-[var(--bg-secondary)] border border-[var(--border)] rounded-2xl p-8 max-w-md w-full mx-4 shadow-md">
        <div className="text-center mb-6">
          <div className="text-xl font-bold text-[var(--text-h)] mb-1">DSX</div>
          <div className="text-sm text-[var(--muted)]">配置你的 AI 助手</div>
        </div>
        <div className="flex justify-center gap-2 mb-6">
          {[1, 2, 3].map(s => (
            <div key={s} className={`w-8 h-8 rounded-full flex items-center justify-center text-xs font-bold transition-colors ${
              s < step ? 'bg-[var(--success)] text-white' : s === step ? 'bg-[var(--accent)] text-white' : 'bg-[var(--bg-tertiary)] text-[var(--muted)]'
            }`}>{s < step ? '✓' : s}</div>
          ))}
        </div>
        <div className="mb-6">
          {step === 1 && (
            <div>
              <label className="block text-sm text-[var(--text-h)] mb-2">API Key</label>
              <input type="password" value={apiKey} onChange={e => setApiKey(e.target.value)}
                placeholder="输入 DeepSeek API Key"
                className="w-full bg-[var(--bg-primary)] border border-[var(--border)] rounded-lg px-3 py-2 text-sm text-[var(--text-h)] font-mono outline-none focus:border-[var(--accent)]" />
              <div className="mt-2 text-xs text-[var(--muted)]">
                在 platform.deepseek.com 获取
              </div>
            </div>
          )}
          {step === 2 && (
            <div>
              <label className="block text-sm text-[var(--text-h)] mb-2">{T.model}</label>
              <input type="text" value={model} onChange={e => setModel(e.target.value)}
                className="w-full bg-[var(--bg-primary)] border border-[var(--border)] rounded-lg px-3 py-2 text-sm text-[var(--text-h)] font-mono outline-none focus:border-[var(--accent)]" />
              <div className="mt-2 text-xs text-[var(--muted)]">
                deepseek-v4-flash · deepseek-v4-pro · deepseek-reasoner-v4
              </div>
            </div>
          )}
          {step === 3 && (
            <div>
              <label className="block text-sm text-[var(--text-h)] mb-2">{T.contextLimit}</label>
              <input type="number" value={contextLimit} onChange={e => setContextLimit(Number(e.target.value))}
                className="w-full bg-[var(--bg-primary)] border border-[var(--border)] rounded-lg px-3 py-2 text-sm text-[var(--text-h)] font-mono outline-none focus:border-[var(--accent)]" />
              <div className="mt-2 text-xs text-[var(--muted)]">
                DeepSeek 最大 1,000,000 tokens
              </div>
            </div>
          )}
        </div>
        {saveError && <div className="mb-3 text-xs text-[var(--error)] bg-[var(--error)]/5 border border-[var(--error)]/20 rounded-lg px-3 py-2">{saveError}</div>}
        <div className="flex gap-3">
          {step > 1 && <button onClick={() => setStep(s => s - 1)} className="flex-1 bg-[var(--bg-tertiary)] text-[var(--text-h)] rounded-lg py-2 text-sm hover:brightness-95">{T.back}</button>}
          <button onClick={() => step < 3 ? setStep(s => s + 1) : finish()} disabled={saving}
            className="flex-1 bg-[var(--accent)] text-white rounded-lg py-2 text-sm font-medium hover:brightness-110 disabled:opacity-40">
            {saving ? '保存中...' : step < 3 ? '下一步' : '开始'}
          </button>
        </div>
      </div>
    </div>
  )
}
