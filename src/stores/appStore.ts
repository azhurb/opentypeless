import { create } from 'zustand'
import type { CredentialNamespace, CredentialStatus } from '../lib/tauri'

export type PipelineState = 'idle' | 'recording' | 'transcribing' | 'polishing' | 'outputting'

export type SttProvider =
  | 'deepgram'
  | 'assemblyai'
  | 'glm-asr'
  | 'openai-whisper'
  | 'groq-whisper'
  | 'siliconflow'
export type LlmProvider =
  | 'zhipu'
  | 'deepseek'
  | 'siliconflow'
  | 'openai'
  | 'gemini'
  | 'moonshot'
  | 'qwen'
  | 'groq'
  | 'claude'
  | 'ollama'
  | 'openrouter'
export type HotkeyMode = 'hold' | 'toggle'
export type Theme = 'light' | 'dark' | 'system'

export interface HistoryEntry {
  id: number
  created_at: string
  app_name: string
  app_type: string
  raw_text: string
  polished_text: string
  language: string | null
  duration_ms: number | null
}

export interface DictionaryEntry {
  id: number
  word: string
  pronunciation: string | null
  source: string
  observed_source: string | null
  frequency_used: number
  last_used: string | null
}

/**
 * Mirrors Rust `storage::AppConfig`. Holds **no secrets** — API keys live in the
 * OS credential vault and are never sent back to the webview. See `keyDrafts`
 * and `credentialStatus` for how the panes deal with keys.
 */
export interface AppConfig {
  stt_provider: SttProvider
  stt_languages: string[]
  llm_provider: LlmProvider
  llm_model: string
  llm_base_url: string
  polish_enabled: boolean
  translate_enabled: boolean
  target_lang: string
  hotkey: string
  hotkey_mode: HotkeyMode
  selected_text_enabled: boolean
  theme: Theme
  auto_start: boolean
  close_to_tray: boolean
  max_recording_seconds: number
  ui_language: string
  capsule_auto_hide: boolean
  learn_from_corrections_enabled: boolean
  history_enabled: boolean
  /** Age limit for history rows, in days. 0 = keep forever. */
  history_retention_days: number
}

export type TestStatus = 'idle' | 'testing' | 'success' | 'error'

/** Unsaved API key text, per namespace. `null` = field untouched. */
export interface KeyDrafts {
  stt: string | null
  llm: string | null
}

export interface CorrectionSuggestion {
  rowId: number
  old: string
  new: string
  autoConfirmMs: number
}

interface AppState {
  // Pipeline
  pipelineState: PipelineState
  setPipelineState: (state: PipelineState) => void

  // Recording
  audioVolume: number
  setAudioVolume: (v: number) => void
  partialTranscript: string
  setPartialTranscript: (t: string) => void
  finalTranscript: string
  setFinalTranscript: (t: string) => void
  polishedText: string
  setPolishedText: (t: string) => void
  appendPolishedChunk: (chunk: string) => void
  recordingDuration: number
  setRecordingDuration: (d: number) => void
  targetApp: string
  setTargetApp: (app: string) => void

  // Config
  config: AppConfig
  configLoaded: boolean
  setConfig: (config: AppConfig) => void
  updateConfig: (partial: Partial<AppConfig>) => void

  // API keys
  //
  // A draft is the key the user is currently typing, held only until Save hands
  // it to the vault. `null` means "the field was not touched", which is what
  // keeps an untouched pane from reading as an unsaved edit — the bug that
  // shipped in 0.5.0 when a placeholder was compared against real config.
  keyDrafts: KeyDrafts
  setKeyDraft: (namespace: CredentialNamespace, value: string | null) => void
  clearKeyDrafts: () => void
  /** Whether the selected providers have a key in the vault. */
  credentialStatus: CredentialStatus
  setCredentialStatus: (status: CredentialStatus) => void

  // History
  history: HistoryEntry[]
  setHistory: (h: HistoryEntry[]) => void

  // Dictionary
  dictionary: DictionaryEntry[]
  setDictionary: (d: DictionaryEntry[]) => void

  // Onboarding
  onboardingCompleted: boolean
  setOnboardingCompleted: (done: boolean) => void
  onboardingStep: number
  setOnboardingStep: (step: number) => void

  // Capsule
  capsuleExpanded: boolean
  setCapsuleExpanded: (expanded: boolean) => void

  // Connection test status
  sttTestStatus: TestStatus
  setSttTestStatus: (s: TestStatus) => void
  llmTestStatus: TestStatus
  setLlmTestStatus: (s: TestStatus) => void

  // Latency benchmark results (ms), null = not yet measured
  sttLatencyMs: number | null
  setSttLatencyMs: (ms: number | null) => void
  llmLatencyMs: number | null
  setLlmLatencyMs: (ms: number | null) => void

  // LLM model list cache (persists across tab switches)
  llmModels: string[]
  setLlmModels: (models: string[]) => void

  // Pipeline error
  pipelineError: string | null
  setPipelineError: (error: string | null) => void

  // Clipboard paste tip — shown when a dictation had no focused target to
  // paste into, so the text was left on the clipboard for manual ⌘V.
  clipboardTip: boolean
  setClipboardTip: (show: boolean) => void

  // Correction suggestion (learn from corrections toast)
  correctionSuggestion: CorrectionSuggestion | null
  setCorrectionSuggestion: (s: CorrectionSuggestion | null) => void

  // macOS Accessibility permission
  accessibilityTrusted: boolean
  setAccessibilityTrusted: (trusted: boolean) => void

  // macOS Microphone permission — `authorized` by default so non-macOS users
  // bypass the banner without an extra round-trip.
  micAuthStatus: 'not_determined' | 'restricted' | 'denied' | 'authorized'
  setMicAuthStatus: (status: 'not_determined' | 'restricted' | 'denied' | 'authorized') => void

  // Context menu
  contextMenuOpen: boolean
  setContextMenuOpen: (open: boolean) => void
  contextMenuReady: boolean
  setContextMenuReady: (ready: boolean) => void

  // Reset recording state
  resetRecording: () => void

  // Config snapshot for dirty detection
  savedConfig: AppConfig | null
  setSavedConfig: (config: AppConfig) => void
  resetConfig: () => void
}

const isMac =
  typeof navigator !== 'undefined' && navigator.platform.toUpperCase().indexOf('MAC') >= 0

const defaultConfig: AppConfig = {
  stt_provider: 'glm-asr',
  stt_languages: [],
  llm_provider: 'openrouter',
  llm_model: 'google/gemini-2.5-flash',
  llm_base_url: 'https://openrouter.ai/api/v1',
  polish_enabled: true,
  translate_enabled: false,
  target_lang: 'en',
  hotkey: isMac ? 'Alt+/' : 'Ctrl+/',
  hotkey_mode: 'hold',
  selected_text_enabled: false,
  theme: 'system',
  auto_start: false,
  close_to_tray: true,
  max_recording_seconds: 30,
  ui_language: 'en',
  capsule_auto_hide: false,
  learn_from_corrections_enabled: false,
  history_enabled: true,
  history_retention_days: 0,
}

export const useAppStore = create<AppState>((set) => ({
  pipelineState: 'idle',
  setPipelineState: (pipelineState) => set({ pipelineState }),

  audioVolume: 0,
  setAudioVolume: (audioVolume) => set({ audioVolume }),
  partialTranscript: '',
  setPartialTranscript: (partialTranscript) => set({ partialTranscript }),
  finalTranscript: '',
  setFinalTranscript: (finalTranscript) => set({ finalTranscript }),
  polishedText: '',
  setPolishedText: (polishedText) => set({ polishedText }),
  appendPolishedChunk: (chunk) => set((s) => ({ polishedText: s.polishedText + chunk })),
  recordingDuration: 0,
  setRecordingDuration: (recordingDuration) => set({ recordingDuration }),
  targetApp: '',
  setTargetApp: (targetApp) => set({ targetApp }),

  config: defaultConfig,
  configLoaded: false,
  // `setConfig` is only ever called with a config that came *from* Rust — the
  // initial `getConfig()` load and the `config:changed` broadcast after a
  // successful save. So it is also the right place to refresh `savedConfig`,
  // which is what tells the UI what the backend actually has on disk (as
  // opposed to `config`, which carries unsaved Settings edits).
  setConfig: (config) => set({ config, savedConfig: config, configLoaded: true }),
  updateConfig: (partial) => set((s) => ({ config: { ...s.config, ...partial } })),

  keyDrafts: { stt: null, llm: null },
  setKeyDraft: (namespace, value) =>
    set((s) => ({ keyDrafts: { ...s.keyDrafts, [namespace]: value } })),
  clearKeyDrafts: () => set({ keyDrafts: { stt: null, llm: null } }),
  credentialStatus: { stt: false, llm: false },
  setCredentialStatus: (credentialStatus) => set({ credentialStatus }),

  history: [],
  setHistory: (history) => set({ history }),

  dictionary: [],
  setDictionary: (dictionary) => set({ dictionary }),

  onboardingCompleted: false,
  setOnboardingCompleted: (onboardingCompleted) => set({ onboardingCompleted }),
  onboardingStep: 0,
  setOnboardingStep: (onboardingStep) => set({ onboardingStep }),

  capsuleExpanded: false,
  setCapsuleExpanded: (capsuleExpanded) => set({ capsuleExpanded }),

  sttTestStatus: 'idle',
  setSttTestStatus: (sttTestStatus) => set({ sttTestStatus }),
  llmTestStatus: 'idle',
  setLlmTestStatus: (llmTestStatus) => set({ llmTestStatus }),

  sttLatencyMs: null,
  setSttLatencyMs: (sttLatencyMs) => set({ sttLatencyMs }),
  llmLatencyMs: null,
  setLlmLatencyMs: (llmLatencyMs) => set({ llmLatencyMs }),

  llmModels: [],
  setLlmModels: (llmModels) => set({ llmModels }),

  pipelineError: null,
  setPipelineError: (pipelineError) => set({ pipelineError }),

  clipboardTip: false,
  setClipboardTip: (clipboardTip) => set({ clipboardTip }),

  correctionSuggestion: null,
  setCorrectionSuggestion: (correctionSuggestion) => set({ correctionSuggestion }),

  accessibilityTrusted: true,
  setAccessibilityTrusted: (accessibilityTrusted) => set({ accessibilityTrusted }),

  micAuthStatus: 'authorized',
  setMicAuthStatus: (micAuthStatus) => set({ micAuthStatus }),

  contextMenuOpen: false,
  setContextMenuOpen: (contextMenuOpen) => set({ contextMenuOpen }),
  contextMenuReady: false,
  setContextMenuReady: (contextMenuReady) => set({ contextMenuReady }),

  resetRecording: () =>
    set({
      audioVolume: 0,
      partialTranscript: '',
      finalTranscript: '',
      polishedText: '',
      recordingDuration: 0,
    }),

  savedConfig: null,
  setSavedConfig: (savedConfig) => set({ savedConfig }),
  resetConfig: () => set((s) => (s.savedConfig ? { config: { ...s.savedConfig } } : {})),
}))
