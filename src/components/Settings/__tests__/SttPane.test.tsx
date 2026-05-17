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
    stt_api_key: '',
    stt_languages: ['en'] as string[],
  },
  updateConfig: vi.fn(),
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
      stt_api_key: '',
      stt_languages: ['en'],
    }
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
    it('renders API key input with current value', () => {
      mockAppStore.config.stt_api_key = 'sk-test123'
      const { container } = render(<SttPane />)
      const input = container.querySelector(
        'input[placeholder="Enter API Key"]',
      ) as HTMLInputElement
      expect(input.value).toBe('sk-test123')
      expect(input.type).toBe('password')
    })

    it('updates config and resets test state when API key changes', () => {
      const { container } = render(<SttPane />)
      const input = container.querySelector(
        'input[placeholder="Enter API Key"]',
      ) as HTMLInputElement

      fireEvent.change(input, { target: { value: 'sk-new-key' } })

      expect(mockAppStore.updateConfig).toHaveBeenCalledWith({ stt_api_key: 'sk-new-key' })
      expect(mockAppStore.setSttTestStatus).toHaveBeenCalledWith('idle')
      expect(mockAppStore.setSttLatencyMs).toHaveBeenCalledWith(null)
    })
  })

  describe('Test button and latency display', () => {
    it('test button is disabled when API key is empty', () => {
      render(<SttPane />)
      const button = screen.getByRole('button', { name: /test/i })
      expect(button).toBeDisabled()
    })

    it('test button is enabled when API key is present', () => {
      mockAppStore.config.stt_api_key = 'sk-test123'
      render(<SttPane />)
      const button = screen.getByRole('button', { name: /test/i })
      expect(button).not.toBeDisabled()
    })

    it('test button is disabled during testing', () => {
      mockAppStore.config.stt_api_key = 'sk-test123'
      mockAppStore.sttTestStatus = 'testing'
      render(<SttPane />)
      const button = screen.getByRole('button', { name: /test/i })
      expect(button).toBeDisabled()
    })

    it('calls benchSttConnection on test button click', async () => {
      const mockBenchStt = vi.mocked(tauri.benchSttConnection)
      mockBenchStt.mockResolvedValue(234)

      mockAppStore.config.stt_api_key = 'sk-test123'
      render(<SttPane />)
      const button = screen.getByRole('button', { name: /test/i })

      fireEvent.click(button)

      await waitFor(() => {
        expect(mockAppStore.setSttTestStatus).toHaveBeenCalledWith('testing')
        expect(mockAppStore.setSttLatencyMs).toHaveBeenCalledWith(null)
      })

      await waitFor(() => {
        expect(mockBenchStt).toHaveBeenCalledWith('sk-test123', 'deepgram')
      })
    })

    it('displays latency in milliseconds when test succeeds', () => {
      mockAppStore.config.stt_api_key = 'sk-test123'
      mockAppStore.sttTestStatus = 'success'
      mockAppStore.sttLatencyMs = 234

      render(<SttPane />)
      expect(screen.getByText('234ms')).toBeInTheDocument()
    })

    it('displays generic success message when latency is null', () => {
      mockAppStore.config.stt_api_key = 'sk-test123'
      mockAppStore.sttTestStatus = 'success'
      mockAppStore.sttLatencyMs = null

      render(<SttPane />)
      expect(screen.getByText('Connection successful')).toBeInTheDocument()
    })

    it('shows error state UI', () => {
      mockAppStore.config.stt_api_key = 'sk-test123'
      mockAppStore.sttTestStatus = 'error'

      render(<SttPane />)
      expect(screen.getByText('Connection failed')).toBeInTheDocument()
    })

    it('does not display latency when status is error', () => {
      mockAppStore.config.stt_api_key = 'sk-test123'
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
