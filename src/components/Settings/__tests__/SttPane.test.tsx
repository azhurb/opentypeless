import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/react'
import { SttPane } from '../SttPane'
import * as tauri from '../../../lib/tauri'

// Mock Tauri
vi.mock('../../../lib/tauri')

// Mock i18n — return the key for translations we don't override so assertions
// can match against either the localized string or the raw key.
vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => {
      const translations: Record<string, string> = {
        'settings.provider': 'Provider',
        'settings.apiKey': 'API Key',
        'settings.test': 'Test',
        'settings.enterApiKey': 'Enter API Key',
        'settings.apiKeySaved': 'Key saved',
        'settings.apiKeyRemove': 'Remove',
        'settings.connectionSuccess': 'Connection successful',
        'settings.connectionFailed': 'Connection failed',
        'settings.storedLocally': 'Stored locally',
        'settings.sttLanguages': 'Recognized Languages',
        'settings.sttLanguagesAutoHint': 'Auto Detect — speak any supported language.',
        'settings.sttLanguagesSingleHint': 'Optimized for this language.',
        'settings.sttLanguagesMultiHint': 'Auto-detect among your selections.',
      }
      return translations[key] || key
    },
  }),
}))

// Mock stores
const mockAppStore = {
  config: {
    stt_provider: 'deepgram' as string,
    stt_languages: ['en'] as string[],
  },
  updateConfig: vi.fn(),
  // API keys are not part of the config any more: `keyDrafts` is what the user
  // is typing, `credentialStatus` is whether the vault already has one.
  keyDrafts: { stt: null as string | null, llm: null as string | null },
  setKeyDraft: vi.fn(),
  credentialStatus: { stt: false, llm: false },
  sttTestStatus: 'idle' as 'idle' | 'testing' | 'success' | 'error',
  setSttTestStatus: vi.fn(),
  sttLatencyMs: null as number | null,
  setSttLatencyMs: vi.fn(),
}

vi.mock('../../../stores/appStore', () => ({
  useAppStore: (selector: any) => {
    if (typeof selector === 'function') {
      return selector(mockAppStore)
    }
    return mockAppStore
  },
}))

describe('SttPane', () => {
  beforeEach(() => {
    mockAppStore.config = {
      stt_provider: 'deepgram',
      stt_languages: ['en'],
    }
    mockAppStore.keyDrafts = { stt: null, llm: null }
    mockAppStore.credentialStatus = { stt: false, llm: false }
    mockAppStore.sttTestStatus = 'idle'
    mockAppStore.sttLatencyMs = null
    vi.clearAllMocks()
  })

  afterEach(() => {
    cleanup()
    vi.clearAllMocks()
  })

  describe('Provider selection', () => {
    it('renders provider dropdown with current value', () => {
      render(<SttPane />)
      const providerSelect = screen.getByRole('combobox')
      expect(providerSelect).toHaveValue('deepgram')
    })

    it('updates config and resets state when provider changes', () => {
      render(<SttPane />)
      const providerSelect = screen.getByRole('combobox')

      fireEvent.change(providerSelect, { target: { value: 'assemblyai' } })

      expect(mockAppStore.updateConfig).toHaveBeenCalledWith({ stt_provider: 'assemblyai' })
      expect(mockAppStore.setSttTestStatus).toHaveBeenCalledWith('idle')
      expect(mockAppStore.setSttLatencyMs).toHaveBeenCalledWith(null)
    })
  })

  describe('API Key input', () => {
    it('renders the draft key as the input value', () => {
      mockAppStore.keyDrafts.stt = 'sk-test123'
      const { container } = render(<SttPane />)
      const input = container.querySelector('input[type="password"]') as HTMLInputElement
      expect(input.value).toBe('sk-test123')
    })

    it('stores a draft, never the config, when the key changes', () => {
      const { container } = render(<SttPane />)
      const input = container.querySelector(
        'input[placeholder="Enter API Key"]',
      ) as HTMLInputElement

      fireEvent.change(input, { target: { value: 'sk-new-key' } })

      expect(mockAppStore.setKeyDraft).toHaveBeenCalledWith('stt', 'sk-new-key')
      // The secret must not reach the config object, which is serialized to
      // settings.json and snapshotted for dirty detection.
      expect(mockAppStore.updateConfig).not.toHaveBeenCalled()
      expect(mockAppStore.setSttTestStatus).toHaveBeenCalledWith('idle')
      expect(mockAppStore.setSttLatencyMs).toHaveBeenCalledWith(null)
    })

    it('shows the saved placeholder over an empty field when a key is in the vault', () => {
      mockAppStore.credentialStatus.stt = true
      const { container } = render(<SttPane />)
      const input = container.querySelector('input[type="password"]') as HTMLInputElement
      // Empty value, not a masked one — a fake value would read as an unsaved
      // edit to the dirty bar.
      expect(input.value).toBe('')
      expect(input.placeholder).toBe('Key saved')
    })

    it('offers Remove only while a saved key is untouched', () => {
      mockAppStore.credentialStatus.stt = true
      const { rerender } = render(<SttPane />)
      expect(screen.getByRole('button', { name: 'Remove' })).toBeInTheDocument()

      mockAppStore.keyDrafts.stt = 'sk-typing'
      rerender(<SttPane />)
      expect(screen.queryByRole('button', { name: 'Remove' })).not.toBeInTheDocument()
    })

    it('Remove stages an empty draft rather than deleting immediately', () => {
      mockAppStore.credentialStatus.stt = true
      render(<SttPane />)

      fireEvent.click(screen.getByRole('button', { name: 'Remove' }))

      // Empty string, not null: the removal is a pending change the Save bar
      // commits, like every other setting.
      expect(mockAppStore.setKeyDraft).toHaveBeenCalledWith('stt', '')
    })
  })

  describe('Test button and latency display', () => {
    it('test button is disabled with no draft and no saved key', () => {
      render(<SttPane />)
      const button = screen.getByRole('button', { name: /test/i })
      expect(button).toBeDisabled()
    })

    it('test button is enabled when a key has been typed', () => {
      mockAppStore.keyDrafts.stt = 'sk-test123'
      render(<SttPane />)
      const button = screen.getByRole('button', { name: /test/i })
      expect(button).not.toBeDisabled()
    })

    it('test button is enabled when only a saved key exists', () => {
      mockAppStore.credentialStatus.stt = true
      render(<SttPane />)
      const button = screen.getByRole('button', { name: /test/i })
      expect(button).not.toBeDisabled()
    })

    it('test button is disabled when the draft was cleared for removal', () => {
      mockAppStore.credentialStatus.stt = true
      mockAppStore.keyDrafts.stt = ''
      render(<SttPane />)
      const button = screen.getByRole('button', { name: /test/i })
      expect(button).toBeDisabled()
    })

    it('test button is disabled during testing', () => {
      mockAppStore.keyDrafts.stt = 'sk-test123'
      mockAppStore.sttTestStatus = 'testing'
      render(<SttPane />)
      const button = screen.getByRole('button', { name: /test/i })
      expect(button).toBeDisabled()
    })

    it('probes the typed key when the field has been edited', async () => {
      const mockBenchStt = vi.mocked(tauri.benchSttConnection)
      mockBenchStt.mockResolvedValue(234)

      mockAppStore.keyDrafts.stt = 'sk-test123'
      render(<SttPane />)

      fireEvent.click(screen.getByRole('button', { name: /test/i }))

      await waitFor(() => {
        expect(mockAppStore.setSttTestStatus).toHaveBeenCalledWith('testing')
        expect(mockAppStore.setSttLatencyMs).toHaveBeenCalledWith(null)
      })

      await waitFor(() => {
        expect(mockBenchStt).toHaveBeenCalledWith('sk-test123', 'deepgram')
      })
    })

    it('probes the stored key when the field was left alone', async () => {
      const mockBenchStt = vi.mocked(tauri.benchSttConnection)
      mockBenchStt.mockResolvedValue(234)

      mockAppStore.credentialStatus.stt = true
      render(<SttPane />)

      fireEvent.click(screen.getByRole('button', { name: /test/i }))

      // `null` tells Rust to read the vault. The webview has no key to send.
      await waitFor(() => {
        expect(mockBenchStt).toHaveBeenCalledWith(null, 'deepgram')
      })
    })

    it('displays latency in milliseconds when test succeeds', () => {
      mockAppStore.keyDrafts.stt = 'sk-test123'
      mockAppStore.sttTestStatus = 'success'
      mockAppStore.sttLatencyMs = 234

      render(<SttPane />)
      expect(screen.getByText('234ms')).toBeInTheDocument()
    })

    it('displays generic success message when latency is null', () => {
      mockAppStore.keyDrafts.stt = 'sk-test123'
      mockAppStore.sttTestStatus = 'success'
      mockAppStore.sttLatencyMs = null

      render(<SttPane />)
      expect(screen.getByText('Connection successful')).toBeInTheDocument()
    })

    it('shows error state UI', () => {
      mockAppStore.keyDrafts.stt = 'sk-test123'
      mockAppStore.sttTestStatus = 'error'

      render(<SttPane />)
      expect(screen.getByText('Connection failed')).toBeInTheDocument()
    })

    it('does not display latency when status is error', () => {
      mockAppStore.keyDrafts.stt = 'sk-test123'
      mockAppStore.sttTestStatus = 'error'
      mockAppStore.sttLatencyMs = 234

      render(<SttPane />)
      expect(screen.queryByText('234ms')).not.toBeInTheDocument()
      expect(screen.getByText('Connection failed')).toBeInTheDocument()
    })
  })

  describe('Language multi-select', () => {
    it('renders one chip per supported language with selected state from config', () => {
      mockAppStore.config.stt_languages = ['en', 'de']
      render(<SttPane />)
      const en = screen.getByRole('button', { name: /English/i })
      const de = screen.getByRole('button', { name: /Deutsch/i })
      const fr = screen.getByRole('button', { name: /Français/i })
      expect(en).toHaveAttribute('aria-pressed', 'true')
      expect(de).toHaveAttribute('aria-pressed', 'true')
      expect(fr).toHaveAttribute('aria-pressed', 'false')
    })

    it('adds a language when an unselected chip is clicked', () => {
      mockAppStore.config.stt_languages = ['en']
      render(<SttPane />)
      const de = screen.getByRole('button', { name: /Deutsch/i })
      fireEvent.click(de)
      expect(mockAppStore.updateConfig).toHaveBeenCalledWith({
        stt_languages: ['en', 'de'],
      })
    })

    it('removes a language when a selected chip is clicked', () => {
      mockAppStore.config.stt_languages = ['en', 'de']
      render(<SttPane />)
      const en = screen.getByRole('button', { name: /English/i })
      fireEvent.click(en)
      expect(mockAppStore.updateConfig).toHaveBeenCalledWith({
        stt_languages: ['de'],
      })
    })

    it('shows the auto-detect hint when no languages are selected', () => {
      mockAppStore.config.stt_languages = []
      render(<SttPane />)
      expect(screen.getByText(/Auto Detect/i)).toBeInTheDocument()
    })

    it('shows the single-language hint when one language is selected', () => {
      mockAppStore.config.stt_languages = ['en']
      render(<SttPane />)
      expect(screen.getByText(/Optimized for this language/i)).toBeInTheDocument()
    })

    it('shows the multi-language hint when 2+ languages are selected', () => {
      mockAppStore.config.stt_languages = ['en', 'de']
      render(<SttPane />)
      expect(screen.getByText(/Auto-detect among your selections/i)).toBeInTheDocument()
    })
  })
})
