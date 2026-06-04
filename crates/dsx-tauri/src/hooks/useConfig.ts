// ── useConfig Hook ──
// Configuration load/save + model fetching.

import { useState, useCallback, useEffect } from 'react'
import { api, type ConfigData, type ConfigInput } from '../bridge/tauri'

interface UseConfigReturn {
  config: ConfigData | null
  loading: boolean
  load: () => Promise<void>
  save: (input: ConfigInput) => Promise<void>
  update: (field: string, value: string) => Promise<void>
  fetchModels: (apiKey: string, baseUrl: string) => Promise<string[]>
  checkDone: boolean
}

export function useConfig(): UseConfigReturn {
  const [config, setConfig] = useState<ConfigData | null>(null)
  const [loading, setLoading] = useState(true)
  const [checkDone, setCheckDone] = useState(false)

  const load = useCallback(async () => {
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
  }, [])

  const save = useCallback(async (input: ConfigInput) => {
    await api.saveConfig(input)
    const cfg = await api.loadConfig()
    setConfig(cfg)
  }, [])

  const update = useCallback(async (field: string, value: string) => {
    await api.updateConfig(field, value)
  }, [])

  const fetchModels = useCallback(async (apiKey: string, baseUrl: string): Promise<string[]> => {
    try {
      return await api.fetchModels(apiKey, baseUrl)
    } catch (e) {
      throw e
    }
  }, [])

  useEffect(() => { load() }, [load])

  return { config, loading, load, save, update, fetchModels, checkDone }
}
