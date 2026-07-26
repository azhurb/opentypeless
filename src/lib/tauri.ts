import { invoke } from '@tauri-apps/api/core'
import type { AppConfig, HistoryEntry, DictionaryEntry } from '../stores/appStore'

// Pipeline commands
export async function startRecording(): Promise<void> {
  return invoke('start_recording')
}

export async function stopRecording(): Promise<void> {
  return invoke('stop_recording')
}

export async function abortRecording(): Promise<void> {
  return invoke('abort_recording')
}

// Config commands
export async function getConfig(): Promise<AppConfig> {
  return invoke('get_config')
}

export async function updateConfig(config: AppConfig): Promise<void> {
  return invoke('update_config', { config })
}

// Credentials
//
// API keys live in the OS credential vault, never in `settings.json` and never
// in this store. Secrets travel one way only — the frontend can write one and
// ask whether one exists, but it cannot read one back.

export type CredentialNamespace = 'stt' | 'llm'

/**
 * Whether a provider has a key — three states, not two.
 *
 * `unreadable` means the credential store refused or failed the read (on macOS,
 * declining the Keychain prompt does this). It must stay distinct from
 * `missing`: rendering it as "no key" invites the user to retype or remove a
 * credential that is actually fine.
 */
export type KeyPresence = 'saved' | 'saved_unencrypted' | 'missing' | 'unreadable'

export interface CredentialStatus {
  stt: KeyPresence
  llm: KeyPresence
}

/** Whether the two currently selected providers have a key saved. */
export async function getCredentialStatus(
  sttProvider: string,
  llmProvider: string,
): Promise<CredentialStatus> {
  return invoke('get_credential_status', { sttProvider, llmProvider })
}

/** Save a key for one provider. An empty `apiKey` removes it. */
export async function setApiKey(
  namespace: CredentialNamespace,
  provider: string,
  apiKey: string,
): Promise<void> {
  return invoke('set_api_key', { namespace, provider, apiKey })
}

// Connection test
//
// `apiKey` is the *unsaved* key the user is typing. Pass it to probe a
// candidate before saving (onboarding, or re-testing after a paste); pass
// `null` to probe whatever is already in the vault. It is never persisted as a
// side effect of testing.
export async function testSttConnection(apiKey: string | null, provider: string): Promise<boolean> {
  return invoke('test_stt_connection', { apiKey, provider })
}

export async function testLlmConnection(
  apiKey: string | null,
  provider: string,
  baseUrl: string,
  model: string,
): Promise<boolean> {
  return invoke('test_llm_connection', { apiKey, provider, baseUrl, model })
}

// Latency benchmark — returns round-trip time in milliseconds
export async function benchSttConnection(apiKey: string | null, provider: string): Promise<number> {
  return invoke('bench_stt_connection', { apiKey, provider })
}

export async function benchLlmConnection(
  apiKey: string | null,
  provider: string,
  baseUrl: string,
  model: string,
): Promise<number> {
  return invoke('bench_llm_connection', { apiKey, provider, baseUrl, model })
}

// LLM models
export async function fetchLlmModels(
  apiKey: string | null,
  provider: string,
  baseUrl: string,
): Promise<string[]> {
  return invoke('fetch_llm_models', { apiKey, provider, baseUrl })
}

// Hotkey
export async function updateHotkey(hotkey: string): Promise<void> {
  return invoke('update_hotkey', { hotkey })
}

export async function pauseHotkey(): Promise<void> {
  return invoke('pause_hotkey')
}

export async function resumeHotkey(): Promise<void> {
  return invoke('resume_hotkey')
}

// History
export async function getHistory(limit: number, offset: number): Promise<HistoryEntry[]> {
  return invoke('get_history', { limit, offset })
}

export async function clearHistory(): Promise<void> {
  return invoke('clear_history')
}

// Dictionary
export async function getDictionary(): Promise<DictionaryEntry[]> {
  return invoke('get_dictionary')
}

export async function addDictionaryEntry(
  word: string,
  pronunciation: string | null,
): Promise<void> {
  return invoke('add_dictionary_entry', { word, pronunciation })
}

export async function removeDictionaryEntry(id: number): Promise<void> {
  return invoke('remove_dictionary_entry', { id })
}

// Corrections
export async function correctionUndo(rowId: number): Promise<void> {
  return invoke('correction_undo', { rowId })
}

// Auto-start
export async function setAutoStart(enabled: boolean): Promise<void> {
  return invoke('set_auto_start', { enabled })
}

// macOS Accessibility permission
export async function checkAccessibilityPermission(): Promise<boolean> {
  return invoke('check_accessibility_permission')
}

export async function requestAccessibilityPermission(): Promise<boolean> {
  return invoke('request_accessibility_permission')
}

// macOS Microphone permission
export type MicAuthStatus = 'not_determined' | 'restricted' | 'denied' | 'authorized'

export async function checkMicrophonePermission(): Promise<MicAuthStatus> {
  return invoke('check_microphone_permission')
}

export async function requestMicrophonePermission(): Promise<boolean> {
  return invoke('request_microphone_permission')
}

// Onboarding persistence via tauri-plugin-store
export async function loadOnboardingCompleted(): Promise<boolean> {
  try {
    const { load } = await import('@tauri-apps/plugin-store')
    const store = await load('settings.json')
    const val = await store.get<boolean>('onboarding_completed')
    return val === true
  } catch {
    return false
  }
}

export async function saveOnboardingCompleted(): Promise<void> {
  try {
    const { load } = await import('@tauri-apps/plugin-store')
    const store = await load('settings.json')
    await store.set('onboarding_completed', true)
  } catch (e) {
    console.error('Failed to persist onboarding state:', e)
  }
}
