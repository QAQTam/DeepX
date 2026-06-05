// ── useConfig Hook (SolidJS) ──
// Configuration load/save + model fetching.

import { createSignal, onMount } from 'solid-js'
import { api, type ConfigData, type ConfigInput } from '../bridge/tauri'

interface UseConfigReturn {
  readonly config: ConfigData | null
  readonly loading: boolean
  load: () => Promise<void>
  save: (input: ConfigInput) => Promise<void>
  update: (field: string, value: string) => Promise<void>
  fetchModels: (apiKey: string, baseUrl: string) => Promise<string[]>
  readonly checkDone: boolean
}

export function useConfig(): UseConfigReturn {
  const [_config, setConfig] = createSignal<ConfigData | null>(null)
  const [_loading, setLoading] = createSignal(true)
  const [_checkDone, setCheckDone] = createSignal(false)

  const load = async () => {
    try {
      setLoading(true)
      const ok = await api.checkConfig()
      setCheckDone(!!ok)
      if (ok) {
        const cfg = await api.loadConfig()
        setConfig(cfg)
      }
    } catch {
      setCheckDone(false)
    } finally {
      setLoading(false)
    }
  }

  const save = async (input: ConfigInput) => {
    await api.saveConfig(input)
    const cfg = await api.loadConfig()
    setConfig(cfg)
  }

  const update = async (field: string, value: string) => {
    await api.updateConfig(field, value)
  }

  const fetchModels = async (apiKey: string, baseUrl: string): Promise<string[]> => {
    return await api.fetchModels(apiKey, baseUrl)
  }

  onMount(() => { load() })

  return {
    get config() { return _config() },
    get loading() { return _loading() },
    load, save, update, fetchModels,
    get checkDone() { return _checkDone() },
  }
}
