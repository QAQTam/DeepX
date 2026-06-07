// ── ConfigWizard ──
// First-run 3-step setup wizard.

import { createSignal } from 'solid-js'
import { api } from '../bridge/tauri'
import { tt } from '../i18n'
import { Button, Input } from './shared'

interface ConfigWizardProps {
  onDone: () => void
}

export function ConfigWizard(props: ConfigWizardProps) {
  const [step, setStep] = createSignal(1)
  const [apiKey, setApiKey] = createSignal('')
  const [model, setModel] = createSignal('deepseek-v4-flash')
  const [contextLimit, setContextLimit] = createSignal(1000000)
  const [saving, setSaving] = createSignal(false)
  const [saveError, setSaveError] = createSignal('')

  const finish = async () => {
    setSaving(true)
    setSaveError('')
    try {
      await api.saveConfig({
        apiKey: apiKey(), baseUrl: 'https://api.deepseek.com', model: model(), contextLimit: contextLimit(),
        maxTokens: 16384, maxToolRounds: 10, providerId: 'deepseek', endpoint: 'openai', reasoningEffort: 'high', lang: 'zh', context7ApiKey: '',
      })
      props.onDone()
    } catch (e: any) {
      setSaving(false)
      setSaveError(String(e))
    }
  }

  const finishLabel = () => step() < 3 ? tt('common.next') : tt('common.start')

  return (
    <div class="h-full flex items-center justify-center bg-[var(--bg-primary)] selection:bg-[var(--accent-light)]">
      <div class="bg-[var(--bg-secondary)] border border-[var(--border)] rounded-2xl p-8 max-w-md w-full mx-4 shadow-md transition-theme">
        <div class="text-center mb-6">
          <div class="text-xl font-bold text-[var(--text-h)] mb-1">DSX</div>
          <div class="text-sm text-[var(--muted)]">{tt('settings.wizardTitle')}</div>
        </div>

        {/* Step indicator */}
        <div class="flex justify-center gap-2 mb-6">
          {[1, 2, 3].map(s => (
            <div class={`w-8 h-8 rounded-full flex items-center justify-center text-xs font-bold transition-colors ${
              s < step() ? 'bg-[var(--success)] text-white'
              : s === step() ? 'bg-[var(--accent)] text-white'
              : 'bg-[var(--bg-tertiary)] text-[var(--muted)]'
            }`}>
              {s < step() ? '✓' : s}
            </div>
          ))}
        </div>

        {/* Step 1: API Key */}
        {step() === 1 && (
          <div class="space-y-4">
            <p class="text-sm text-[var(--text)]">{tt('settings.wizardStep1')}</p>
            <Input
              label={tt('settings.apiKey')}
              type="password"
              value={apiKey()}
              onInput={(e) => setApiKey(e.currentTarget.value)}
              placeholder="sk-..."
              autofocus
            />
          </div>
        )}

        {/* Step 2: Model */}
        {step() === 2 && (
          <div class="space-y-4">
            <p class="text-sm text-[var(--text)]">{tt('settings.wizardStep2')}</p>
            <Input
              label={tt('settings.model')}
              value={model()}
              onInput={(e) => setModel(e.currentTarget.value)}
            />
          </div>
        )}

        {/* Step 3: Context Limit */}
        {step() === 3 && (
          <div class="space-y-4">
            <p class="text-sm text-[var(--text)]">{tt('settings.wizardStep3')}</p>
            <Input
              label={tt('settings.contextLimit')}
              type="number"
              value={String(contextLimit())}
              onInput={(e) => setContextLimit(Number(e.currentTarget.value))}
            />
          </div>
        )}

        {/* Error */}
        {saveError() && (
          <div class="mt-4 p-2 bg-[var(--error)]/10 border border-[var(--error)]/30 rounded-lg text-xs text-[var(--error)]">
            {saveError()}
          </div>
        )}

        {/* Actions */}
        <div class="flex gap-2 mt-6">
          {step() > 1 && (
            <Button variant="secondary" onClick={() => setStep(s => s - 1)} class="flex-1">
              {tt('common.previous')}
            </Button>
          )}
          <Button variant="primary" onClick={() => step() < 3 ? setStep(s => s + 1) : finish()} loading={saving()} class="flex-1">
            {saving() ? tt('settings.saving') : finishLabel()}
          </Button>
        </div>
      </div>
    </div>
  )
}
