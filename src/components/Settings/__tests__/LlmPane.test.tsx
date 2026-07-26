import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/react'
import { LlmPane } from '../LlmPane'
import * as tauri from '../../../lib/tauri'
import type { KeyPresence } from '../../../lib/tauri'

// Mock Tauri
vi.mock('../../../lib/tauri')

// Mock i18n
vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, params?: any) => {
      const translations: Record<string, string> = {
        'settings.provider': 'Provider',
        'settings.apiKey': 'API Key',
        'settings.model': 'Model',
        'settings.baseUrl': 'Base URL',
        'settings.test': 'Test',
        'settings.enterApiKey': 'Enter API Key',
        'settings.apiKeySaved': 'Key saved',
        'settings.apiKeyRemove': 'Remove',
        'settings.connectionSuccess': 'Connection successful',
        'settings.connectionFailed': 'Connection failed',
        'settings.storedLocally': 'Stored locally',
        'settings.fetchModels': 'Fetch models',
        'settings.modelsAvailable': `${params?.count || 0} models available`,
        'settings.enableAiPolish': 'Enable AI Polish',
        'settings.translationMode': 'Translation Mode',
        'settings.selectedTextContext': 'Selected Text Context',
        'settings.selectedTextContextDesc': 'Include selected text as context',
        'settings.targetLanguage': 'Target Language',
      }
      return translations[key] || key
    },
  }),
}))

// Mock stores - must be done before importing the component
const mockAppStore = {
  config: {
    llm_provider: 'openai' as string,
    llm_base_url: 'https://api.openai.com/v1',
    llm_model: 'gpt-4o-mini',
    polish_enabled: true,
    translate_enabled: false,
    selected_text_enabled: false,
    target_lang: 'en',
  },
  updateConfig: vi.fn(),
  // API keys are not part of the config any more: `keyDrafts` is what the user
  // is typing, `credentialStatus` is whether the vault already has one.
  keyDrafts: { stt: null as string | null, llm: null as string | null },
  setKeyDraft: vi.fn(),
  credentialStatus: { stt: 'missing' as KeyPresence, llm: 'missing' as KeyPresence },
  llmTestStatus: 'idle' as 'idle' | 'testing' | 'success' | 'error',
  setLlmTestStatus: vi.fn(),
  llmLatencyMs: null as number | null,
  setLlmLatencyMs: vi.fn(),
  llmModels: [] as string[],
  setLlmModels: vi.fn(),
}

vi.mock('../../../stores/appStore', () => ({
  useAppStore: (selector: any) => {
    if (typeof selector === 'function') {
      return selector(mockAppStore)
    }
    return mockAppStore
  },
}))

describe('LlmPane', () => {
  beforeEach(() => {
    // Reset mock store state
    mockAppStore.config = {
      llm_provider: 'openai',
      llm_base_url: 'https://api.openai.com/v1',
      llm_model: 'gpt-4o-mini',
      polish_enabled: true,
      translate_enabled: false,
      selected_text_enabled: false,
      target_lang: 'en',
    }
    mockAppStore.keyDrafts = { stt: null, llm: null }
    mockAppStore.credentialStatus = { stt: 'missing', llm: 'missing' }
    mockAppStore.llmTestStatus = 'idle'
    mockAppStore.llmLatencyMs = null
    mockAppStore.llmModels = []
  })

  afterEach(() => {
    cleanup()
    vi.clearAllMocks()
  })

  describe('Provider selection', () => {
    it('renders provider dropdown with current value', () => {
      render(<LlmPane />)
      const selects = screen.getAllByRole('combobox')
      const providerSelect = selects[0] // First select is provider
      expect(providerSelect).toHaveValue('openai')
    })

    it('updates config and resets state when provider changes', () => {
      render(<LlmPane />)
      const selects = screen.getAllByRole('combobox')
      const providerSelect = selects[0]

      fireEvent.change(providerSelect, { target: { value: 'anthropic' } })

      expect(mockAppStore.updateConfig).toHaveBeenCalled()
      expect(mockAppStore.setLlmTestStatus).toHaveBeenCalledWith('idle')
      expect(mockAppStore.setLlmLatencyMs).toHaveBeenCalledWith(null)
      expect(mockAppStore.setLlmModels).toHaveBeenCalledWith([])
    })
  })

  describe('API Key input', () => {
    it('renders the draft key as the input value', () => {
      mockAppStore.keyDrafts.llm = 'sk-test123'
      render(<LlmPane />)
      const input = screen.getByPlaceholderText('Enter API Key') as HTMLInputElement
      expect(input.value).toBe('sk-test123')
      expect(input.type).toBe('password')
    })

    it('stores a draft, never the config, when the key changes', () => {
      render(<LlmPane />)
      const input = screen.getByPlaceholderText('Enter API Key')

      fireEvent.change(input, { target: { value: 'sk-new-key' } })

      expect(mockAppStore.setKeyDraft).toHaveBeenCalledWith('llm', 'sk-new-key')
      // The secret must not reach the config object, which is serialized to
      // settings.json and snapshotted for dirty detection.
      expect(mockAppStore.updateConfig).not.toHaveBeenCalled()
      expect(mockAppStore.setLlmTestStatus).toHaveBeenCalledWith('idle')
      expect(mockAppStore.setLlmLatencyMs).toHaveBeenCalledWith(null)
    })

    it('shows the saved placeholder over an empty field when a key is in the vault', () => {
      mockAppStore.credentialStatus.llm = 'saved'
      render(<LlmPane />)
      const input = screen.getByPlaceholderText('Key saved') as HTMLInputElement
      expect(input.value).toBe('')
    })

    it('Remove stages an empty draft rather than deleting immediately', () => {
      mockAppStore.credentialStatus.llm = 'saved'
      render(<LlmPane />)

      fireEvent.click(screen.getByRole('button', { name: 'Remove' }))

      expect(mockAppStore.setKeyDraft).toHaveBeenCalledWith('llm', '')
    })
  })

  describe('Test button and latency display', () => {
    it('test button is disabled when API key is empty', () => {
      render(<LlmPane />)
      const button = screen.getByRole('button', { name: /test/i })
      expect(button).toBeDisabled()
    })

    it('test button is enabled when API key is present', () => {
      mockAppStore.keyDrafts.llm = 'sk-test123'
      render(<LlmPane />)
      const button = screen.getByRole('button', { name: /test/i })
      expect(button).not.toBeDisabled()
    })

    it('shows loading state during test', () => {
      mockAppStore.keyDrafts.llm = 'sk-test123'
      mockAppStore.llmTestStatus = 'testing'
      render(<LlmPane />)
      const button = screen.getByRole('button', { name: /test/i })
      expect(button).toBeDisabled()
    })

    it('calls benchLlmConnection on test button click', async () => {
      const mockBenchLlm = vi.mocked(tauri.benchLlmConnection)
      mockBenchLlm.mockResolvedValue(187)

      mockAppStore.keyDrafts.llm = 'sk-test123'
      render(<LlmPane />)
      const button = screen.getByRole('button', { name: /test/i })

      fireEvent.click(button)

      await waitFor(() => {
        expect(mockAppStore.setLlmTestStatus).toHaveBeenCalledWith('testing')
        expect(mockAppStore.setLlmLatencyMs).toHaveBeenCalledWith(null)
      })

      await waitFor(() => {
        expect(mockBenchLlm).toHaveBeenCalledWith(
          'sk-test123',
          'openai',
          'https://api.openai.com/v1',
          'gpt-4o-mini',
        )
      })
    })

    it('probes the stored key when the field was left alone', async () => {
      const mockBenchLlm = vi.mocked(tauri.benchLlmConnection)
      mockBenchLlm.mockResolvedValue(187)

      mockAppStore.credentialStatus.llm = 'saved'
      render(<LlmPane />)

      fireEvent.click(screen.getByRole('button', { name: /test/i }))

      // `null` tells Rust to read the vault. The webview has no key to send.
      await waitFor(() => {
        expect(mockBenchLlm).toHaveBeenCalledWith(
          null,
          'openai',
          'https://api.openai.com/v1',
          'gpt-4o-mini',
        )
      })
    })

    it('displays latency in milliseconds when test succeeds', () => {
      mockAppStore.keyDrafts.llm = 'sk-test123'
      mockAppStore.llmTestStatus = 'success'
      mockAppStore.llmLatencyMs = 187

      render(<LlmPane />)
      expect(screen.getByText('187ms')).toBeInTheDocument()
    })

    it('displays generic success message when latency is null', () => {
      mockAppStore.keyDrafts.llm = 'sk-test123'
      mockAppStore.llmTestStatus = 'success'
      mockAppStore.llmLatencyMs = null

      render(<LlmPane />)
      expect(screen.getByText('Connection successful')).toBeInTheDocument()
    })

    it('shows error state UI', () => {
      mockAppStore.keyDrafts.llm = 'sk-test123'
      mockAppStore.llmTestStatus = 'error'

      render(<LlmPane />)
      expect(screen.getByText('Connection failed')).toBeInTheDocument()
    })
  })

  describe('Model input', () => {
    it('updates config and resets latency when model changes', () => {
      render(<LlmPane />)
      const input = screen.getByPlaceholderText('e.g. gpt-4o-mini')

      fireEvent.change(input, { target: { value: 'gpt-4o' } })

      expect(mockAppStore.updateConfig).toHaveBeenCalledWith({ llm_model: 'gpt-4o' })
      expect(mockAppStore.setLlmLatencyMs).toHaveBeenCalledWith(null)
    })

    it('displays available models count', () => {
      mockAppStore.llmModels = ['gpt-4o', 'gpt-4o-mini', 'gpt-3.5-turbo']

      render(<LlmPane />)
      expect(screen.getByText('3 models available')).toBeInTheDocument()
    })
  })

  describe('Base URL input', () => {
    it('updates config when base URL changes', () => {
      render(<LlmPane />)
      const input = screen.getByPlaceholderText('https://open.bigmodel.cn/api/paas/v4')

      fireEvent.change(input, { target: { value: 'https://custom.api.com/v1' } })

      expect(mockAppStore.updateConfig).toHaveBeenCalledWith({
        llm_base_url: 'https://custom.api.com/v1',
      })
    })
  })

  describe('Feature toggles', () => {
    it('shows target language selector when translation is enabled', () => {
      mockAppStore.config.translate_enabled = true

      render(<LlmPane />)
      expect(screen.getByText('Target Language')).toBeInTheDocument()
    })
  })
})
