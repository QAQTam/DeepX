// ── Typed Tauri IPC Bridge ──
// Every invoke() call is type-safe. No <any> needed in components.

import { invoke } from '@tauri-apps/api/core'

// ── Return types ──

export interface ConfigData {
  api_key?: string
  base_url?: string
  model?: string
  context_limit?: number
  max_tokens?: number
  provider_id?: string
  protocol?: string
  endpoint?: string
  reasoning_effort?: string
  lang?: string
  context7_api_key?: string
}

export interface ProviderInfo {
  id: string
  display: string
  endpoints: EndpointInfo[]
}

export interface EndpointInfo {
  id: string
  display: string
  protocol: string
  base_url: string
  default_model: string
  models: string[]
}

export interface BalanceResult {
  is_available: boolean
  balance_infos?: Array<{
    currency: string
    total_balance: string
    granted_balance: string
    topped_up_balance: string
  }>
}

export interface AgentStartResponse {
  seed?: string
  sessions: SessionInfo[]
}

export interface SessionInfo {
  seed: string
  date?: string
  model?: string
  message_count?: number
}

export interface DirectoryEntry {
  name: string
  is_dir: boolean
  size: number
}

export interface ScanResult {
  entries: DirectoryEntry[]
}

export interface SessionMessages {
  messages: Array<{
    role: 'user' | 'assistant'
    content: string
    reasoning?: string
    tool_cards?: unknown[]
  }>
}

// ── Input types ──

export interface ConfigInput {
  apiKey: string
  baseUrl: string
  model: string
  contextLimit: number
  maxTokens: number
  providerId: string
  endpoint: string
  reasoningEffort: string
  lang: string
  context7ApiKey: string
}

// ── API object (all 18 commands) ──

export const api = {
  // Config
  checkConfig:     ()              => invoke<boolean>('check_config'),
  loadConfig:      ()              => invoke<ConfigData>('load_config'),
  saveConfig:      (c: ConfigInput) => invoke<void>('save_config', {
                                        apiKey: c.apiKey,
                                        baseUrl: c.baseUrl,
                                        model: c.model,
                                        contextLimit: c.contextLimit,
                                        maxTokens: c.maxTokens,
                                        providerId: c.providerId,
                                        endpoint: c.endpoint,
                                        reasoningEffort: c.reasoningEffort,
                                        lang: c.lang,
                                        context7ApiKey: c.context7ApiKey,
                                      }),
  updateConfig:    (field: string, value: string) => invoke<void>('update_config', { field, value }),

  // Models & Balance
  listProviders:   ()              => invoke<ProviderInfo[]>('list_providers'),
  getBalance:      (apiKey: string) => invoke<BalanceResult>('get_balance', { apiKey }),

  // Agent lifecycle
  startAgent:      ()              => invoke<AgentStartResponse>('start_agent'),
  checkAgentStatus: () => invoke<{ running: boolean; seed?: string }>('check_agent_status'),
  stopAgent:       ()              => invoke<void>('stop_agent'),
  resumeAgent:     (seed: string)  => invoke<void>('resume_agent', { seed }),
  createSession:   ()              => invoke<void>('create_session'),
  cancelAgent:     ()              => invoke<void>('cancel_agent'),
  reloadAgent:     ()              => invoke<void>('reload_agent'),

  // Messaging
  sendMessage:     (text: string)  => invoke<void>('send_message', { text }),

  // Sessions
  loadSessionMessages: (seed: string) => invoke<SessionMessages>('load_session_messages', { seed }),
  cmdSessions:     ()              => invoke<SessionInfo[]>('cmd_sessions'),
  deleteSession:   (seed: string)  => invoke<void>('delete_session', { seed }),
  deleteAllSessions: ()            => invoke<void>('delete_all_sessions'),

  // Workspace
  setWorkspace:    (path: string)  => invoke<void>('set_workspace', { path }),
  getWorkspace:    ()              => invoke<string>('get_workspace'),
  scanDirectory:   (path: string)  => invoke<ScanResult>('scan_directory', { path }),
} as const
