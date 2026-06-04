// ── ConfigWizard ──
// First-run 3-step setup wizard.

import { useState, type ChangeEvent } from 'react'
import { api } from '../bridge/tauri'
import { tt } from '../i18n'
import { Button, Input, Select } from './shared'

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

  const finish = async () => {
    setSaving(true)
    setSaveError('')
    try {
      await api.saveConfig({
        apiKey, baseUrl: 'https://api.deepseek.com', model, contextLimit,
        maxTokens: 16384, effort: 'high', lang: 'zh',
      })
      onDone()
    } catch (e: any) {
      setSaving(false)
      setSaveError(String(e))
    }
  }

  const finishLabel = step < 3 ? tt('common.next') : tt('common.start')

  return (
    <div className="h-full flex items-center justify-center bg-[var(--bg-primary)] selection:bg-[var(--accent-light)]">
      <div className="bg-[var(--bg-secondary)] border border-[var(--border)] rounded-2xl p-8 max-w-md w-full mx-4 shadow-md transition-theme">
        <div className="text-center mb-6">
          <div className="text-xl font-bold text-[var(--text-h)] mb-1">DSX</div>
          <div className="text-sm text-[var(--muted)]">{tt('settings.wizardTitle')}</div>
        </div>

        {/* Step indicator */}
        <div className="flex justify-center gap-2 mb-6">
          {[1, 2, 3].map(s => (
            <div key={s} className={`w-8 h-8 rounded-full flex items-center justify-center text-xs font-bold transition-colors ${
              s < step ? 'bg-[var(--success)] text-white'
              : s === step ? 'bg-[var(--accent)] text-white'
              : 'bg-[var(--bg-tertiary)] text-[var(--muted)]'
            }`}>{s < step ? '✓' : s}</div>
          ))}
        </div>

        {/* Step content */}
        <div className="mb-6 min-h-[120px]">
          {step === 1 && (
            <Input
              type="password"
              label={tt('settings.apiKey')}
              placeholder={tt('settings.apiKeyPlaceholder')}
              hint={tt('settings.apiKeyHint')}
              value={apiKey}
              onChange={(e: ChangeEvent<HTMLInputElement>) => setApiKey(e.target.value)}
              autoFocus
            />
          )}

          {step === 2 && (
            <Select
              label={tt('settings.model')}
              value={model}
              onChange={(e: ChangeEvent<HTMLSelectElement>) => setModel(e.target.value)}
              options={[
                { value: 'deepseek-v4-flash', label: 'DeepSeek V4 Flash (快速)' },
                { value: 'deepseek-v4', label: 'DeepSeek V4 (标准)' },
                { value: 'deepseek-v4-reasoning', label: 'DeepSeek V4 Reasoning (推理)' },
              ]}
            />
          )}

          {step === 3 && (
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
          )}

          {saveError && (
            <p className="mt-2 text-xs text-[var(--error)]" role="alert">{saveError}</p>
          )}
        </div>

        {/* Navigation */}
        <div className="flex gap-3">
          {step > 1 && (
            <Button variant="secondary" onClick={() => setStep(s => s - 1)} className="flex-1">
              {tt('common.back')}
            </Button>
          )}
          <Button variant="primary" onClick={() => step < 3 ? setStep(s => s + 1) : finish()} loading={saving} className="flex-1">
            {saving ? tt('settings.saving') : finishLabel}
          </Button>
        </div>
      </div>
    </div>
  )
}
