/**
 * MainApp's initial load is the bottleneck for "did onboarding wipe my
 * keys?". The regression we're pinning here: when `onboarding_completed`
 * is false the app used to skip `getConfig()` entirely, leaving Zustand on
 * `defaultConfig` (empty keys). The SttSetupStep / LlmSetupStep then
 * rendered empty inputs, and the save at the end of onboarding wrote those
 * empty values over the user's real keys still sitting in settings.json.
 *
 * The fix loads config unconditionally; this test pins it.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, waitFor, cleanup } from '@testing-library/react'
import { useAppStore } from '../stores/appStore'

afterEach(() => {
  cleanup()
})

// ── Mocks ────────────────────────────────────────────────────────────────
// Stub every Tauri command MainApp uses so the test doesn't hit invoke().

const loadOnboardingCompleted = vi.fn()
const getConfig = vi.fn()
const getHistory = vi.fn()
const getDictionary = vi.fn()
const checkAccessibilityPermission = vi.fn()
const checkMicrophonePermission = vi.fn()
const requestMicrophonePermission = vi.fn()

vi.mock('../lib/tauri', () => ({
  loadOnboardingCompleted: (...a: unknown[]) => loadOnboardingCompleted(...a),
  getConfig: (...a: unknown[]) => getConfig(...a),
  getHistory: (...a: unknown[]) => getHistory(...a),
  getDictionary: (...a: unknown[]) => getDictionary(...a),
  checkAccessibilityPermission: (...a: unknown[]) => checkAccessibilityPermission(...a),
  checkMicrophonePermission: (...a: unknown[]) => checkMicrophonePermission(...a),
  requestMicrophonePermission: (...a: unknown[]) => requestMicrophonePermission(...a),
  // Unused in this test but imported by the sub-tree.
  saveOnboardingCompleted: vi.fn(),
  updateConfig: vi.fn(),
}))

// MainApp pulls in useTauriEvents which uses listen() — stub.
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => undefined),
}))

// useTheme calls window.matchMedia which jsdom doesn't ship; rather than
// stubbing the DOM globally, swap the hook itself out — this test is about
// data loading, not theming.
vi.mock('../hooks/useTheme', () => ({ useTheme: () => 'system' }))

// Heavy / non-essential children — render as no-ops so the test focuses on
// the MainApp load effect. We don't care what these components show.
vi.mock('../components/Onboarding', () => ({
  Onboarding: () => null,
}))
vi.mock('../components/MainLayout', () => ({
  MainLayout: ({ children }: { children: React.ReactNode }) => <>{children}</>,
}))
vi.mock('../components/HomePage', () => ({ HomePage: () => null }))
vi.mock('../components/Settings', () => ({ Settings: () => null }))
vi.mock('../components/History', () => ({ History: () => null }))
vi.mock('../components/Capsule', () => ({ Capsule: () => null }))
vi.mock('../components/Toast', () => ({ ToastContainer: () => null }))

vi.mock('react-i18next', async (importOriginal) => {
  const actual = await importOriginal<typeof import('react-i18next')>()
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string) => key,
      i18n: { language: 'en', changeLanguage: vi.fn() },
    }),
  }
})

import App from '../App'

const MOCK_CONFIG = {
  stt_provider: 'groq-whisper',
  stt_api_key: 'real-stt-key-from-disk',
  stt_language: 'en',
  llm_provider: 'gemini',
  llm_api_key: 'real-llm-key-from-disk',
  llm_model: 'models/gemini-2.5-flash',
  llm_base_url: 'https://generativelanguage.googleapis.com/v1beta/openai',
  polish_enabled: true,
  translate_enabled: false,
  target_lang: 'en',
  hotkey: 'Meta+/',
  hotkey_mode: 'hold' as const,
  selected_text_enabled: false,
  theme: 'system' as const,
  auto_start: false,
  close_to_tray: true,
  max_recording_seconds: 30,
  ui_language: 'en',
  capsule_auto_hide: false,
  learn_from_corrections_enabled: false,
}

function resetAll() {
  useAppStore.setState(useAppStore.getInitialState())
  loadOnboardingCompleted.mockReset()
  getConfig.mockReset().mockResolvedValue(MOCK_CONFIG)
  getHistory.mockReset().mockResolvedValue([])
  getDictionary.mockReset().mockResolvedValue([])
  checkAccessibilityPermission.mockReset().mockResolvedValue(true)
  checkMicrophonePermission.mockReset().mockResolvedValue('authorized')
  requestMicrophonePermission.mockReset().mockResolvedValue(true)
  // Mount the main-window route.
  window.location.hash = ''
  // Pretend we're on Linux so the mac permission branch is skipped — the
  // test is about config-load, not permission probing.
  Object.defineProperty(window.navigator, 'platform', {
    value: 'Linux x86_64',
    configurable: true,
  })
}

describe('MainApp initial load — config preservation', () => {
  beforeEach(() => {
    resetAll()
  })

  it('loads config even when onboarding is NOT completed', async () => {
    // Pre-this-fix: getConfig() was guarded by `if (done)` and skipped here,
    // leaving the store on defaultConfig with empty keys. Onboarding would
    // then save its empty inputs over the on-disk keys.
    loadOnboardingCompleted.mockResolvedValue(false)

    render(<App />)

    await waitFor(() => {
      expect(getConfig).toHaveBeenCalled()
      expect(useAppStore.getState().config.stt_api_key).toBe('real-stt-key-from-disk')
      expect(useAppStore.getState().config.llm_api_key).toBe('real-llm-key-from-disk')
    })
  })

  it('also loads config when onboarding IS completed (no regression)', async () => {
    loadOnboardingCompleted.mockResolvedValue(true)

    render(<App />)

    await waitFor(() => {
      expect(getConfig).toHaveBeenCalled()
      expect(useAppStore.getState().config.stt_api_key).toBe('real-stt-key-from-disk')
    })
  })

  it('skips history/dictionary load while onboarding is in progress', async () => {
    loadOnboardingCompleted.mockResolvedValue(false)

    render(<App />)

    await waitFor(() => {
      expect(getConfig).toHaveBeenCalled()
    })
    // History + dictionary are only relevant once onboarding is done. Avoid
    // the I/O while the user is still entering credentials.
    expect(getHistory).not.toHaveBeenCalled()
    expect(getDictionary).not.toHaveBeenCalled()
  })

  it('loads history + dictionary once onboarding is completed', async () => {
    loadOnboardingCompleted.mockResolvedValue(true)

    render(<App />)

    await waitFor(() => {
      expect(getHistory).toHaveBeenCalled()
      expect(getDictionary).toHaveBeenCalled()
    })
  })
})
