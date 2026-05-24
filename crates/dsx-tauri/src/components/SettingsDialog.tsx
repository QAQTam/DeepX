import { useState, useEffect } from 'preact/compat'
import { useForm, type SubmitHandler } from 'react-hook-form'
import { zodResolver } from '@hookform/resolvers/zod'
import { z } from 'zod'
import * as Dialog from '@radix-ui/react-dialog'
import { motion } from 'framer-motion'
import { invoke } from '@tauri-apps/api/core'
import { T } from '../i18n'

const settingsSchema = z.object({
  apiKey: z.string(),
  baseUrl: z.string().min(1, 'Base URL is required'),
  contextLimit: z.number().min(1000, 'Min 1000'),
  maxTokens: z.number().min(1, 'Min 1'),
  autoMode: z.boolean(),
  promptLang: z.enum(['zh', 'en']),
  effort: z.enum(['high', 'max']),
})

type SettingsForm = z.infer<typeof settingsSchema>

interface PhaseConfig {
  model: string
  context_limit: number
  max_tokens: number
  effort: string
}

interface SettingsDialogProps {
  onClose: () => void
}

export function SettingsDialog({ onClose }: SettingsDialogProps) {
  const [models, setModels] = useState<string[]>([])
  const [fetching, setFetching] = useState(false)
  const [fetchError, setFetchError] = useState('')
  const [phaseConfigs, setPhaseConfigs] = useState<Record<string, PhaseConfig>>({
    plan: { model: 'deepseek-v4-pro', context_limit: 1000000, max_tokens: 4096, effort: 'max' },
    coding: { model: 'deepseek-v4-flash', context_limit: 1000000, max_tokens: 16384, effort: 'high' },
    debug: { model: 'deepseek-v4-pro', context_limit: 1000000, max_tokens: 8192, effort: 'high' },
  })

  const { register, handleSubmit, setValue, watch, getValues, formState: { errors } } = useForm<SettingsForm>({
    resolver: zodResolver(settingsSchema),
    defaultValues: {
      apiKey: '',
      baseUrl: 'https://api.deepseek.com/anthropic',
      contextLimit: 1000000,
      maxTokens: 8192,
      autoMode: true,
      promptLang: 'zh',
      effort: 'high',
    },
  })

  const autoMode = watch('autoMode')

  useEffect(() => {
    invoke<any>('load_config').then(cfg => {
      if (cfg.api_key && cfg.api_key !== 'null') setValue('apiKey', cfg.api_key)
      if (cfg.base_url) setValue('baseUrl', cfg.base_url)
      if (cfg.context_limit) setValue('contextLimit', cfg.context_limit)
      if (cfg.max_tokens) setValue('maxTokens', cfg.max_tokens)
      if (cfg.auto_mode !== undefined) setValue('autoMode', cfg.auto_mode)
      if (cfg.prompt_lang) setValue('promptLang', cfg.prompt_lang)
      if (cfg.effort) setValue('effort', cfg.effort)
      if (cfg.phase_configs) {
        try { setPhaseConfigs(typeof cfg.phase_configs === 'string' ? JSON.parse(cfg.phase_configs) : cfg.phase_configs) } catch { /* keep defaults */ }
      }
      let cached = cfg.cached_models_deepseek || cfg.cached_models
      if (typeof cached === 'string') try { cached = JSON.parse(cached) } catch { /* ignore */ }
      if (Array.isArray(cached)) setModels(cached)
    }).catch(() => {})
  }, [setValue])

  const fetchModels = () => {
    const { apiKey, baseUrl } = getValues()
    setFetching(true); setFetchError('')
    invoke<string[]>('fetch_models', { apiKey, baseUrl }).then(list => {
      setModels(list)
      invoke('update_config', { field: 'cached_models_deepseek', value: JSON.stringify(list.slice(0, 5)) }).catch(() => {})
    }).catch(e => { setFetchError(String(e)); setModels([]) }).finally(() => setFetching(false))
  }

  const save = (data: SettingsForm) => {
    invoke('save_config', {
      apiKey: data.apiKey, baseUrl: data.baseUrl,
      contextLimit: data.contextLimit, maxTokens: data.maxTokens,
      autoMode: data.autoMode, promptLang: data.promptLang, effort: data.effort,
    }).then(() => {
      invoke('update_config', { field: 'phase_configs', value: JSON.stringify(phaseConfigs) }).then(() => {
        invoke('reload_agent').catch(() => {})
      })
      onClose()
    }).catch(() => {
      invoke('update_config', { field: 'api_key', value: data.apiKey })
      invoke('update_config', { field: 'base_url', value: data.baseUrl })
      invoke('update_config', { field: 'context_limit', value: String(data.contextLimit) })
      invoke('update_config', { field: 'max_tokens', value: String(data.maxTokens) })
      invoke('update_config', { field: 'auto_mode', value: String(data.autoMode) })
      invoke('update_config', { field: 'prompt_lang', value: data.promptLang })
      invoke('update_config', { field: 'effort', value: data.effort })
      invoke('update_config', { field: 'phase_configs', value: JSON.stringify(phaseConfigs) }).then(() => {
        invoke('reload_agent').catch(() => {})
      })
      onClose()
    })
  }

  const phases = [
    { key: 'plan', label: 'Plan', color: 'var(--warning)' },
    { key: 'coding', label: 'Coding', color: 'var(--text-h)' },
    { key: 'debug', label: 'Debug', color: 'var(--error)' },
  ]

  return (
    <Dialog.Root open onOpenChange={(open) => { if (!open) onClose() }}>
      <Dialog.Portal>
        <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: 0.15 }}>
          <Dialog.Overlay className="fixed inset-0 bg-black/30 z-50" />
        </motion.div>
        <Dialog.Content className="fixed inset-0 z-50 flex items-center justify-center">
          <motion.form initial={{ opacity: 0, scale: 0.95, y: 10 }} animate={{ opacity: 1, scale: 1, y: 0 }}
            transition={{ duration: 0.2 }} onSubmit={handleSubmit(save as SubmitHandler<SettingsForm>)}
            className="bg-[var(--bg-primary)] border border-[var(--border)] rounded-2xl p-6 max-w-2xl w-full mx-4 shadow-md max-h-[90vh] overflow-y-auto">
            <Dialog.Title className="text-sm font-bold text-[var(--text-h)] mb-4">{T.settings}</Dialog.Title>

            <div className="grid grid-cols-2 gap-3 mb-3">
              <div>
                <label className="block text-xs text-[var(--muted)] mb-1">DeepSeek API Key</label>
                <input type="password" {...register('apiKey')}
                  placeholder="输入 DeepSeek API Key"
                  className="w-full bg-[var(--bg-tertiary)] border border-[var(--border)] rounded-lg px-3 py-1.5 text-xs text-[var(--text-h)] font-mono outline-none focus:border-[var(--accent)]" />
              </div>
              <div>
                <label className="block text-xs text-[var(--muted)] mb-1">Base URL</label>
                <input type="text" {...register('baseUrl')}
                  className="w-full bg-[var(--bg-tertiary)] border border-[var(--border)] rounded-lg px-3 py-1.5 text-xs text-[var(--text-h)] font-mono outline-none focus:border-[var(--accent)]" />
                {errors.baseUrl && <div className="text-xs text-[var(--error)] mt-1">{errors.baseUrl.message}</div>}
                <div className="text-xs text-[var(--muted)] mt-1">DeepSeek Anthropic 兼容端点</div>
              </div>
            </div>

            <div className="grid grid-cols-2 gap-3 mb-3">
              <div className="flex items-end">
                <button type="button" onClick={fetchModels}
                  disabled={fetching}
                  className="bg-[var(--bg-tertiary)] border border-[var(--border)] text-[var(--text-h)] rounded-lg px-3 py-1.5 text-xs hover:bg-[var(--bg-hover)] disabled:opacity-40 transition-colors">
                  {fetching ? '获取中...' : '获取模型列表'}
                </button>
              </div>
              <div className="flex items-end">
                <div className="text-xs text-[var(--muted)]">
                  {models.length > 0 ? `已获取 ${models.length} 个模型，在聊天输入框上方切换` : ''}
                </div>
              </div>
            </div>
            {fetchError && <div className="text-xs text-[var(--error)] mb-3 -mt-2">{fetchError}</div>}

            <div className="grid grid-cols-2 gap-3 mb-3">
              <div>
                <label className="block text-xs text-[var(--muted)] mb-1">Context Limit</label>
                <input type="number" {...register('contextLimit', { valueAsNumber: true })}
                  className="w-full bg-[var(--bg-tertiary)] border border-[var(--border)] rounded-lg px-3 py-1.5 text-xs text-[var(--text-h)] font-mono outline-none focus:border-[var(--accent)]" />
              </div>
              <div>
                <label className="block text-xs text-[var(--muted)] mb-1">Max Tokens</label>
                <input type="number" {...register('maxTokens', { valueAsNumber: true })}
                  className="w-full bg-[var(--bg-tertiary)] border border-[var(--border)] rounded-lg px-3 py-1.5 text-xs text-[var(--text-h)] font-mono outline-none focus:border-[var(--accent)]" />
              </div>
            </div>

            <div className="grid grid-cols-2 gap-3 mb-3">
              <div>
                <label className="block text-xs text-[var(--muted)] mb-1">思考强度</label>
                <div className="flex gap-1">
                  {(['high', 'max'] as const).map(e => (
                    <button key={e} type="button" onClick={() => setValue('effort', e)}
                      className={`flex-1 rounded-lg py-1.5 text-xs border transition-all ${watch('effort') === e ? 'bg-[var(--accent-light)] border-[var(--accent)] text-[var(--accent)] font-medium' : 'bg-[var(--bg-tertiary)] border-[var(--border)] text-[var(--text)]'}`}>
                      {e === 'high' ? '高' : '最高'}
                    </button>
                  ))}
                </div>
              </div>
              <div>
                <label className="block text-xs text-[var(--muted)] mb-1">{T.language}</label>
                <div className="flex gap-1">
                  {(['zh', 'en'] as const).map(l => (
                    <button key={l} type="button" onClick={() => setValue('promptLang', l)}
                      className={`flex-1 rounded-lg py-1.5 text-xs border transition-all ${watch('promptLang') === l ? 'bg-[var(--accent-light)] border-[var(--accent)] text-[var(--accent)] font-medium' : 'bg-[var(--bg-tertiary)] border-[var(--border)] text-[var(--text)]'}`}>
                      {l === 'zh' ? '中文' : 'English'}
                    </button>
                  ))}
                </div>
              </div>
            </div>

            <div className="mb-4 p-3 rounded-lg border bg-[var(--bg-secondary)] border-[var(--border)]">
              <label className="flex items-center gap-3 cursor-pointer">
                <div className="flex-1">
                  <div className="text-xs text-[var(--text-h)] font-medium">Auto Mode</div>
                  <div className="text-xs text-[var(--muted)]">自动选择模型和推理阶段</div>
                </div>
                <input type="checkbox" {...register('autoMode')}
                  className="w-9 h-5 rounded-full appearance-none bg-[var(--bg-tertiary)] border border-[var(--border)] checked:bg-[var(--accent)] checked:border-[var(--accent)] relative before:absolute before:w-3.5 before:h-3.5 before:rounded-full before:bg-white before:top-[1px] before:left-[1px] checked:before:left-[17px] before:transition-all cursor-pointer" />
              </label>
            </div>

            {autoMode && (
              <div className="mb-4">
                <div className="text-xs text-[var(--text-h)] font-medium mb-2">Phase Configs</div>
                <div className="border border-[var(--border)] rounded-lg overflow-hidden">
                  <table className="w-full text-[11px]">
                    <thead>
                      <tr className="bg-[var(--bg-tertiary)] text-[var(--muted)]">
                        <th className="text-left px-3 py-1.5 font-medium">Phase</th>
                        <th className="text-left px-3 py-1.5 font-medium">Model</th>
                        <th className="text-left px-3 py-1.5 font-medium">Context</th>
                        <th className="text-left px-3 py-1.5 font-medium">Max Tok</th>
                        <th className="text-left px-3 py-1.5 font-medium">Effort</th>
                      </tr>
                    </thead>
                    <tbody>
                      {phases.map(phase => {
                        const pc = phaseConfigs[phase.key]
                        if (!pc) return null
                        return (
                          <tr key={phase.key} className="border-t border-[var(--border)] hover:bg-[var(--bg-hover)]">
                            <td className="px-3 py-1.5">
                              <div className="flex items-center gap-1.5">
                                <span className="w-2 h-2 rounded-full shrink-0" style={{ background: phase.color }} />
                                <span className="text-[var(--text-h)] font-medium">{phase.label}</span>
                              </div>
                            </td>
                            <td className="px-3 py-1.5">
                              {models.length > 0 ? (
                                <select value={pc.model} onChange={e =>
                                  setPhaseConfigs(prev => ({ ...prev, [phase.key]: { ...prev[phase.key], model: e.target.value } }))
                                }
                                  className="w-full bg-[var(--bg-tertiary)] border border-[var(--border)] rounded px-2 py-1 text-[11px] text-[var(--text-h)] font-mono outline-none focus:border-[var(--accent)]">
                                  {models.map(m => <option key={m} value={m}>{m}</option>)}
                                </select>
                              ) : (
                                <input type="text" value={pc.model} onChange={e =>
                                  setPhaseConfigs(prev => ({ ...prev, [phase.key]: { ...prev[phase.key], model: e.target.value } }))
                                }
                                  className="w-full bg-[var(--bg-tertiary)] border border-[var(--border)] rounded px-2 py-1 text-[11px] text-[var(--text-h)] font-mono outline-none focus:border-[var(--accent)]" />
                              )}
                            </td>
                            <td className="px-3 py-1.5">
                              <input type="number" value={pc.context_limit} onChange={e =>
                                setPhaseConfigs(prev => ({ ...prev, [phase.key]: { ...prev[phase.key], context_limit: Number(e.target.value) } }))
                              }
                                className="w-20 bg-[var(--bg-tertiary)] border border-[var(--border)] rounded px-2 py-1 text-[11px] text-[var(--text-h)] font-mono outline-none focus:border-[var(--accent)]" />
                            </td>
                            <td className="px-3 py-1.5">
                              <input type="number" value={pc.max_tokens} onChange={e =>
                                setPhaseConfigs(prev => ({ ...prev, [phase.key]: { ...prev[phase.key], max_tokens: Number(e.target.value) } }))
                              }
                                className="w-20 bg-[var(--bg-tertiary)] border border-[var(--border)] rounded px-2 py-1 text-[11px] text-[var(--text-h)] font-mono outline-none focus:border-[var(--accent)]" />
                            </td>
                            <td className="px-3 py-1.5">
                              <select value={pc.effort} onChange={e =>
                                setPhaseConfigs(prev => ({ ...prev, [phase.key]: { ...prev[phase.key], effort: e.target.value } }))
                              }
                                className="w-full bg-[var(--bg-tertiary)] border border-[var(--border)] rounded px-2 py-1 text-[11px] text-[var(--text-h)] font-mono outline-none focus:border-[var(--accent)]">
                                <option value="high">高</option>
                                <option value="max">最高</option>
                              </select>
                            </td>
                          </tr>
                        )
                      })}
                    </tbody>
                  </table>
                </div>
              </div>
            )}

            <div className="flex gap-3">
              <Dialog.Close asChild>
                <button type="button"
                  className="flex-1 bg-[var(--bg-tertiary)] text-[var(--text-h)] rounded-lg py-2 text-sm hover:brightness-95">{T.cancel}</button>
              </Dialog.Close>
              <button type="submit"
                className="flex-1 bg-[var(--accent)] text-white rounded-lg py-2 text-sm font-medium hover:brightness-110">{T.save}</button>
            </div>
          </motion.form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
