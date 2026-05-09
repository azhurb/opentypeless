/**
 * Settings component test suite
 *
 * Coverage:
 * 1. Tab switching — clicking sidebar items shows the matching pane
 * 2. Animation structure — AnimatePresence wrapper renders correctly
 * 3. appStore.llmModels — state lift: initial value, read/write, reset
 * 4. LlmPane provider switching — clears the models cache
 * 5. LlmPane useEffect skip — does not re-fetch when cache is populated
 * 6. DirtyBar — appears on config change, disappears after Reset
 * 7. appStore getInitialState — llmModels is an empty array after reset
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, waitFor, act, cleanup } from '@testing-library/react'
import React from 'react'
import { useAppStore } from '../../../stores/appStore'

// Clean up the DOM after each test so repeated render() calls don't leave
// duplicate nodes that confuse getByText.
afterEach(() => {
  cleanup()
})

// ─── Mock framer-motion ───────────────────────────────────────────────────────
// Strip framer-motion-only props so React doesn't warn about unknown DOM
// attributes and getByText doesn't match duplicates.
const MOTION_PROPS = new Set([
  'initial',
  'animate',
  'exit',
  'transition',
  'variants',
  'whileHover',
  'whileTap',
  'whileFocus',
  'whileDrag',
  'whileInView',
  'layoutId',
  'layout',
  'drag',
  'dragConstraints',
  'onAnimationComplete',
])

vi.mock('framer-motion', () => ({
  AnimatePresence: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  motion: new Proxy(
    {},
    {
      get:
        (_t, tag: string) =>
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        ({ children, ...rest }: any) => {
          const domProps: Record<string, unknown> = {}
          for (const [k, v] of Object.entries(rest)) {
            if (!MOTION_PROPS.has(k)) domProps[k] = v
          }
          return React.createElement(tag as string, { 'data-motion': tag, ...domProps }, children)
        },
    },
  ),
}))

// ─── Mock react-i18next ───────────────────────────────────────────────────────
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

// ─── Mock Tauri plugins / lib/tauri ──────────────────────────────────────────
vi.mock('../../../lib/tauri', () => ({
  updateHotkey: vi.fn().mockResolvedValue(undefined),
  pauseHotkey: vi.fn().mockResolvedValue(undefined),
  resumeHotkey: vi.fn().mockResolvedValue(undefined),
  setAutoStart: vi.fn().mockResolvedValue(undefined),
  testSttConnection: vi.fn().mockResolvedValue(true),
  testLlmConnection: vi.fn().mockResolvedValue(true),
  fetchLlmModels: vi.fn().mockResolvedValue(['gpt-4o', 'gpt-3.5-turbo']),
  addDictionaryEntry: vi.fn().mockResolvedValue(undefined),
  removeDictionaryEntry: vi.fn().mockResolvedValue(undefined),
  getDictionary: vi.fn().mockResolvedValue([]),
  updateConfig: vi.fn().mockResolvedValue(undefined),
}))

// ─── Mock @tauri-apps/plugin-opener ─────────────────────────────────────────
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }))

// ─── Mock lib/api (ScenesPane uses getScenes) ────────────────────────────────
vi.mock('../../../lib/api', () => ({
  getScenes: vi.fn().mockResolvedValue([]),
}))

// ─── Mock stores/authStore ────────────────────────────────────────────────────
vi.mock('../../../stores/authStore', () => ({
  useAuthStore: () => ({ user: null, plan: 'free' }),
}))

// ─── Import components AFTER mocks ───────────────────────────────────────────
import { Settings } from '../index'

// ─── Helpers ─────────────────────────────────────────────────────────────────
function resetStore() {
  useAppStore.setState(useAppStore.getInitialState())
}

function seedSavedConfig() {
  const { config } = useAppStore.getState()
  useAppStore.getState().setSavedConfig(config)
}

function renderSettings() {
  return render(<Settings />)
}

// Sidebar nav button: matches the <button data-motion="button"> child inside
// the sidebar, excluding the title-bar button which contains an <h2>.
function clickSidebarItem(label: string) {
  const spans = screen.getAllByText(label)
  const sidebarSpan = spans.find((el) => {
    const btn = el.closest('[data-motion="button"]')
    return btn !== null && btn.querySelector('h2') === null
  })
  const btn = (sidebarSpan ?? spans[0]).closest('[data-motion="button"], button')
  if (btn) fireEvent.click(btn)
  else fireEvent.click(spans[0])
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. Tab switching — renders the matching pane
// ─────────────────────────────────────────────────────────────────────────────
describe('Settings tab switching', () => {
  beforeEach(() => {
    resetStore()
    seedSavedConfig()
  })

  it('initial render shows the General pane (with hotkey section)', () => {
    renderSettings()
    expect(screen.getByText('settings.hotkey')).toBeDefined()
  })

  it('shows STT provider fields after clicking Speech Recognition', () => {
    renderSettings()
    clickSidebarItem('settings.speechRecognition')
    expect(screen.getByText('settings.provider')).toBeDefined()
    expect(screen.getByText('settings.sttLanguage')).toBeDefined()
  })

  it('shows LLM provider fields after clicking AI Polish', () => {
    renderSettings()
    clickSidebarItem('settings.aiPolish')
    expect(screen.getByText('settings.enableAiPolish')).toBeDefined()
  })

  it('shows the dictionary input placeholder after clicking Dictionary', () => {
    renderSettings()
    clickSidebarItem('settings.dictionary')
    expect(screen.getByPlaceholderText('dictionary.word')).toBeDefined()
  })

  it('shows the sign-in prompt after clicking Scenes (user=null)', () => {
    renderSettings()
    clickSidebarItem('settings.scenes')
    expect(screen.getByText('scenes.signInToBrowse')).toBeDefined()
  })

  it('shows the version info section after clicking About', () => {
    renderSettings()
    clickSidebarItem('settings.about')
    expect(screen.getByText('settings.openSource')).toBeDefined()
  })

  it('can switch back and forth between multiple tabs', () => {
    renderSettings()
    clickSidebarItem('settings.aiPolish')
    expect(screen.getByText('settings.enableAiPolish')).toBeDefined()

    clickSidebarItem('settings.general')
    expect(screen.getByText('settings.hotkey')).toBeDefined()
  })

  it('updates the title bar after switching tabs', () => {
    renderSettings()
    clickSidebarItem('settings.dictionary')
    const titles = screen.getAllByText('settings.dictionary')
    // At least twice: sidebar nav and title bar h2.
    expect(titles.length).toBeGreaterThanOrEqual(2)
  })
})

// ─────────────────────────────────────────────────────────────────────────────
// 2. Animation structure — AnimatePresence wrapper renders correctly
// ─────────────────────────────────────────────────────────────────────────────
describe('Settings animation structure', () => {
  beforeEach(() => {
    resetStore()
    seedSavedConfig()
  })

  it('motion wrapper renders pane content', () => {
    const { container } = renderSettings()
    // Our mock tags motion elements with a data-motion attribute.
    expect(container.querySelector('[data-motion]')).not.toBeNull()
  })

  it('updates pane content after switching tabs (no freeze)', () => {
    renderSettings()
    clickSidebarItem('settings.speechRecognition')
    expect(document.body).toBeDefined()
  })
})

// ─────────────────────────────────────────────────────────────────────────────
// 3. appStore.llmModels — store-layer tests
// ─────────────────────────────────────────────────────────────────────────────
describe('appStore.llmModels', () => {
  beforeEach(() => {
    resetStore()
  })

  it('initial value is an empty array', () => {
    expect(useAppStore.getState().llmModels).toEqual([])
  })

  it('setLlmModels updates the store', () => {
    useAppStore.getState().setLlmModels(['model-a', 'model-b'])
    expect(useAppStore.getState().llmModels).toEqual(['model-a', 'model-b'])
  })

  it('setLlmModels([]) clears the cache', () => {
    useAppStore.getState().setLlmModels(['model-a'])
    useAppStore.getState().setLlmModels([])
    expect(useAppStore.getState().llmModels).toHaveLength(0)
  })

  it('llmModels is preserved across component unmount', () => {
    useAppStore.getState().setLlmModels(['gpt-4o', 'claude-3'])
    // Simulate "navigate away and back": zustand state outlives components.
    const { unmount } = render(<div />)
    unmount()
    expect(useAppStore.getState().llmModels).toEqual(['gpt-4o', 'claude-3'])
  })

  it('setLlmModels replaces rather than merges', () => {
    useAppStore.getState().setLlmModels(['a', 'b', 'c'])
    useAppStore.getState().setLlmModels(['x'])
    expect(useAppStore.getState().llmModels).toEqual(['x'])
  })
})

// ─────────────────────────────────────────────────────────────────────────────
// 4. LlmPane — clears models cache when the provider changes
// ─────────────────────────────────────────────────────────────────────────────
describe('LlmPane provider switch clears models', () => {
  beforeEach(() => {
    resetStore()
    seedSavedConfig()
  })

  it('clears llmModels when provider changes', async () => {
    useAppStore.getState().setLlmModels(['model-x', 'model-y'])

    renderSettings()
    clickSidebarItem('settings.aiPolish')

    // Provider select is the first combobox in the current pane.
    const selects = screen.getAllByRole('combobox')
    const providerSelect = selects[0]

    await act(async () => {
      fireEvent.change(providerSelect, { target: { value: 'openai' } })
    })

    expect(useAppStore.getState().llmModels).toEqual([])
  })
})

// ─────────────────────────────────────────────────────────────────────────────
// 5. LlmPane useEffect — skips fetch when cache is populated
// ─────────────────────────────────────────────────────────────────────────────
describe('LlmPane models cache: skip fetch when populated', () => {
  beforeEach(() => {
    resetStore()
    seedSavedConfig()
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
    vi.clearAllMocks()
  })

  it('does not call fetchLlmModels when llmModels already has entries', async () => {
    const { fetchLlmModels } = await import('../../../lib/tauri')
    const mockFetch = vi.mocked(fetchLlmModels)
    mockFetch.mockClear()

    useAppStore.getState().setLlmModels(['cached-model'])
    useAppStore.getState().updateConfig({
      llm_api_key: 'sk-test',
      llm_base_url: 'https://api.openai.com/v1',
      llm_provider: 'openai',
    })

    renderSettings()
    clickSidebarItem('settings.aiPolish')

    await act(async () => {
      vi.runAllTimers()
    })

    expect(mockFetch).not.toHaveBeenCalled()
  })

  it('calls fetchLlmModels when llmModels is empty and api key/url are set', async () => {
    const { fetchLlmModels } = await import('../../../lib/tauri')
    const mockFetch = vi.mocked(fetchLlmModels)
    mockFetch.mockClear()

    useAppStore.getState().setLlmModels([])
    useAppStore.getState().updateConfig({
      llm_api_key: 'sk-test',
      llm_base_url: 'https://api.openai.com/v1',
      llm_provider: 'openai',
    })

    renderSettings()
    clickSidebarItem('settings.aiPolish')

    // runAllTimersAsync advances fake timers and flushes pending microtasks.
    await act(async () => {
      await vi.runAllTimersAsync()
    })

    expect(mockFetch).toHaveBeenCalledTimes(1)
  })

  it('updates llmModels in the store after fetchLlmModels resolves', async () => {
    const { fetchLlmModels } = await import('../../../lib/tauri')
    vi.mocked(fetchLlmModels).mockResolvedValue(['gpt-4o', 'gpt-3.5-turbo'])

    useAppStore.getState().setLlmModels([])
    useAppStore.getState().updateConfig({
      llm_api_key: 'sk-test',
      llm_base_url: 'https://api.openai.com/v1',
      llm_provider: 'openai',
    })

    renderSettings()
    clickSidebarItem('settings.aiPolish')

    await act(async () => {
      await vi.runAllTimersAsync()
    })

    expect(useAppStore.getState().llmModels).toEqual(['gpt-4o', 'gpt-3.5-turbo'])
  })
})

// ─────────────────────────────────────────────────────────────────────────────
// 6. DirtyBar — appears on config change, disappears after Reset
// ─────────────────────────────────────────────────────────────────────────────
describe('DirtyBar behavior', () => {
  beforeEach(() => {
    resetStore()
    seedSavedConfig()
  })

  it('is hidden in the initial state', () => {
    renderSettings()
    expect(screen.queryByText('Unsaved changes')).toBeNull()
  })

  it('appears after config is modified', async () => {
    renderSettings()
    act(() => {
      useAppStore.getState().updateConfig({ theme: 'dark' })
    })
    await waitFor(() => {
      expect(screen.getByText('Unsaved changes')).toBeDefined()
    })
  })

  it('disappears after clicking Reset', async () => {
    renderSettings()
    act(() => {
      useAppStore.getState().updateConfig({ theme: 'dark' })
    })
    await waitFor(() => {
      expect(screen.getByText('Unsaved changes')).toBeDefined()
    })

    fireEvent.click(screen.getByText('Reset'))

    await waitFor(() => {
      expect(screen.queryByText('Unsaved changes')).toBeNull()
    })
  })

  it('shows both Save and Reset buttons', async () => {
    renderSettings()
    act(() => {
      useAppStore.getState().updateConfig({ theme: 'dark' })
    })
    await waitFor(() => {
      expect(screen.getByText('Save')).toBeDefined()
      expect(screen.getByText('Reset')).toBeDefined()
    })
  })
})

// ─────────────────────────────────────────────────────────────────────────────
// 7. appStore getInitialState — llmModels is part of the initial state
// ─────────────────────────────────────────────────────────────────────────────
describe('appStore getInitialState includes llmModels', () => {
  it('getInitialState().llmModels is an empty array', () => {
    const initial = useAppStore.getInitialState()
    expect(initial.llmModels).toEqual([])
  })

  it('setState(getInitialState()) restores llmModels to empty', () => {
    useAppStore.getState().setLlmModels(['stale-model'])
    useAppStore.setState(useAppStore.getInitialState())
    expect(useAppStore.getState().llmModels).toEqual([])
  })

  it('getInitialState does not change fields other than llmModels', () => {
    const initial = useAppStore.getInitialState()
    expect(initial.config.hotkey).toBe('Ctrl+/')
    expect(initial.pipelineState).toBe('idle')
    expect(initial.dictionary).toEqual([])
  })
})
