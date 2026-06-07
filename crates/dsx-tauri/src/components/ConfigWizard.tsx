// ── ConfigWizard ──
// First-run 3-step setup wizard. Uses provider registry for defaults.

import { createSignal, onMount } from 'solid-js'
import { api, type ProviderInfo, type EndpointInfo } from '../bridge/tauri'
import { tt } from '../i18n'
import { Button, Input, Select } from './shared'

interface ConfigWizardProps {
  onDone: () => void
}

export function ConfigWizard(props: ConfigWizardProps) {
  const [step, setStep] = createSignal(1)
  const [apiKey, setApiKey] = createSignal('')
  const [providerId, setProviderId] = createSignal('deepseek')
  const [providers, setProviders] = createSignal<ProviderInfo[]>([])
  const [model, setModel] = createSignal('')
  const [contextLimit, setContextLimit] = createSignal(1000000)
  const [saving, setSaving] = createSignal(false)
  const [saveError, setSaveError] = createSignal('')

  onMount(() => {
    api.listProviders().then(list => {
      setProviders(list)
      if (list.length > 0) {
        setProviderId(list[0].id)
        setModel(list[0].endpoints[0]?.default_model ?? '')
      }
    }).catch(() => {})
  })

  const currentEndpoints = () => {
    const p = providers().find(p => p.id === providerId())
    return p?.endpoints ?? []
  }

  const currentEndpoint = (): EndpointInfo | undefined =>
    currentEndpoints()[0]

  const baseUrl = () => currentEndpoints()[0]?.base_url ?? ''
  const endpointId = () => currentEndpoints()[0]?.id ?? 'openai'

  const finish = async () => {
    setSaving(true)
    setSaveError('')
    try {
      await api.saveConfig({
        apiKey: apiKey(), baseUrl: baseUrl(), model: model(), contextLimit: contextLimit(),
        maxTokens: 16384, providerId: providerId(), endpoint: endpointId(), reasoningEffort: 'high', lang: 'zh', context7ApiKey: '',
      })
      props.onDone()
    } catch (e: any) {
      setSaving(false)
      setSaveError(String(e))
    }
  }

  const finishLabel = () => step() < getTotalSteps() ? tt('common.next') : tt('common.start')
  const getTotalSteps = () => 4

  return (
    <div class="h-full flex items-center justify-center bg-[var(--bg-primary)] selection:bg-[var(--accent-light)]" onKeyDown={(e) => { if (e.key === 'Escape') props.onDone() }}>
      <div class="bg-[var(--bg-secondary)] border border-[var(--border)] rounded-2xl p-8 max-w-md w-full mx-4 shadow-md transition-theme">
        <div class="text-center mb-6">
          <div class="text-xl font-bold text-[var(--text-h)] mb-1">DeepX</div>
          <div class="text-sm text-[var(--muted)]">{tt('settings.wizardTitle')}</div>
        </div>

        {/* Step indicator */}
        <div class="flex justify-center gap-2 mb-6">
          {[1, 2, 3, 4].map(s => (
            <div class={`w-8 h-8 rounded-full flex items-center justify-center text-xs font-bold transition-colors ${
              s < step() ? 'bg-[var(--success)] text-white'
              : s === step() ? 'bg-[var(--accent)] text-white'
              : 'bg-[var(--bg-tertiary)] text-[var(--muted)]'
            }`}>
              {s < step() ? '✓' : s}
            </div>
          ))}
        </div>

        {/* Step 1: Provider */}
        {step() === 1 && (
          <div class="space-y-4">
            <p class="text-sm text-[var(--text)]">Select your AI provider</p>
            <Select
              label="Provider"
              options={providers().map(p => ({ value: p.id, label: p.display }))}
              value={providerId()}
              onChange={(e) => {
                setProviderId(e.currentTarget.value)
                const p = providers().find(p => p.id === e.currentTarget.value)
                const ep = p?.endpoints[0]
                if (ep) setModel(ep.default_model)
              }}
            />
          </div>
        )}

        {/* Step 2: API Key */}
        {step() === 2 && (
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

        {/* Step 3: Model */}
        {step() === 3 && (
          <div class="space-y-4">
            <p class="text-sm text-[var(--text)]">{tt('settings.wizardStep2')}</p>
            <Select
              label={`Model (${providerId()})`}
              options={(currentEndpoint()?.models ?? []).map(m => ({ value: m, label: m }))}
              value={model()}
              onChange={(e) => setModel(e.currentTarget.value)}
            />
          </div>
        )}

        {/* Step 4: Context Limit */}
        {step() === 4 && (
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
        <div class="flex gap-3 mt-6">
          {step() > 1 && (
            <Button variant="secondary" size="lg" onClick={() => setStep(s => s - 1)} class="flex-1">
              {tt('common.previous')}
            </Button>
          )}
          <Button variant="primary" size="lg" onClick={() => step() < getTotalSteps() ? setStep(s => s + 1) : finish()} loading={saving()} class="flex-1">
            {saving() ? tt('settings.saving') : finishLabel()}
          </Button>
        </div>
      </div>
    </div>
  )
}
