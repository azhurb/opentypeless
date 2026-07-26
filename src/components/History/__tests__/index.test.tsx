import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { History } from '../index'
import * as tauri from '../../../lib/tauri'
import type { HistoryEntry } from '../../../stores/appStore'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}))

vi.mock('../../../lib/tauri')

const mockAppStore: {
  history: HistoryEntry[]
  setHistory: (h: HistoryEntry[]) => void
  // The notice reads the persisted config, so the mock must expose `savedConfig`.
  config: { history_enabled: boolean }
  savedConfig: { history_enabled: boolean } | null
} = {
  history: [],
  setHistory: vi.fn(),
  config: { history_enabled: true },
  savedConfig: { history_enabled: true },
}

vi.mock('../../../stores/appStore', () => ({
  useAppStore: (selector: any) => selector(mockAppStore),
}))

function entry(overrides: Partial<HistoryEntry> = {}): HistoryEntry {
  return {
    id: 1,
    created_at: '2026-05-17T10:30:00',
    app_name: 'Slack',
    app_type: 'Chat',
    raw_text: 'hallo welt',
    polished_text: 'Hallo Welt.',
    language: null,
    duration_ms: 1200,
    ...overrides,
  }
}

describe('History row language badge', () => {
  beforeEach(() => {
    mockAppStore.history = []
    mockAppStore.config = { history_enabled: true }
    mockAppStore.savedConfig = { history_enabled: true }
    vi.clearAllMocks()
  })

  afterEach(() => {
    cleanup()
  })

  it('renders an uppercase language badge when entry.language is set', () => {
    mockAppStore.history = [entry({ id: 1, language: 'de' })]
    render(<History />)
    expect(screen.getByText('DE')).toBeInTheDocument()
  })

  it('omits the language badge when entry.language is null', () => {
    mockAppStore.history = [entry({ id: 2, language: null })]
    render(<History />)
    // The polished text still renders, but no language code badge.
    expect(screen.getByText('Hallo Welt.')).toBeInTheDocument()
    expect(screen.queryByTestId('history-language-badge')).not.toBeInTheDocument()
  })

  it('renders a separate badge per row', () => {
    mockAppStore.history = [
      entry({ id: 1, language: 'de', polished_text: 'Hallo Welt.' }),
      entry({ id: 2, language: 'en', polished_text: 'Hello world.' }),
    ]
    render(<History />)
    expect(screen.getByText('DE')).toBeInTheDocument()
    expect(screen.getByText('EN')).toBeInTheDocument()
  })
})

describe('History saving-disabled notice', () => {
  beforeEach(() => {
    mockAppStore.history = []
    mockAppStore.config = { history_enabled: true }
    mockAppStore.savedConfig = { history_enabled: true }
    vi.clearAllMocks()
  })

  afterEach(() => {
    cleanup()
  })

  it('stays hidden while history saving is enabled', () => {
    mockAppStore.history = [entry()]
    render(<History />)
    expect(screen.queryByTestId('history-saving-disabled')).not.toBeInTheDocument()
  })

  it('shows the notice while keeping stored entries listed', () => {
    mockAppStore.savedConfig = { history_enabled: false }
    mockAppStore.history = [entry({ polished_text: 'Hallo Welt.' })]
    render(<History />)
    expect(screen.getByTestId('history-saving-disabled')).toBeInTheDocument()
    // Disabling saving must not hide what is already stored.
    expect(screen.getByText('Hallo Welt.')).toBeInTheDocument()
    expect(screen.getByText('history.clearAll')).toBeInTheDocument()
  })

  it('drops the "press your hotkey" empty-state hint when saving is off', () => {
    mockAppStore.savedConfig = { history_enabled: false }
    render(<History />)
    expect(screen.queryByText('history.noHistoryHint')).not.toBeInTheDocument()
    expect(screen.getByTestId('history-saving-disabled')).toBeInTheDocument()
  })

  it('ignores an unsaved toggle — the backend is still recording until Save', () => {
    // `config` carries unsaved Settings edits. Trusting it here would tell the
    // user nothing is being recorded while Rust is still writing every dictation.
    mockAppStore.config = { history_enabled: false }
    mockAppStore.savedConfig = { history_enabled: true }
    render(<History />)
    expect(screen.queryByTestId('history-saving-disabled')).not.toBeInTheDocument()
  })

  it('falls back to config before the persisted config has loaded', () => {
    mockAppStore.savedConfig = null
    mockAppStore.config = { history_enabled: false }
    render(<History />)
    expect(screen.getByTestId('history-saving-disabled')).toBeInTheDocument()
  })
})

// Regression: "Clear All History" did nothing on macOS. It was gated on
// `window.confirm`, which WKWebView silently resolves to false because wry
// implements no `runJavaScriptConfirmPanelWithMessage:` UI-delegate method — so
// the handler always took its early return. These tests deliberately do NOT stub
// any browser dialog primitive; jsdom's unimplemented `confirm` reproduces the
// macOS behaviour, so a mocked one would assert a contract the platform doesn't
// provide.
describe('History clear-all confirmation', () => {
  beforeEach(() => {
    mockAppStore.history = [entry()]
    mockAppStore.config = { history_enabled: true }
    mockAppStore.savedConfig = { history_enabled: true }
    vi.clearAllMocks()
  })

  afterEach(() => {
    cleanup()
  })

  it('opens a confirmation instead of doing nothing', () => {
    render(<History />)
    fireEvent.click(screen.getByText('history.clearAll'))
    expect(screen.getByTestId('confirm-dialog')).toBeInTheDocument()
    expect(tauri.clearHistory).not.toHaveBeenCalled()
  })

  it('clears history once the confirmation is accepted', async () => {
    vi.mocked(tauri.clearHistory).mockResolvedValue(undefined)
    render(<History />)
    fireEvent.click(screen.getByText('history.clearAll'))
    fireEvent.click(screen.getByTestId('confirm-dialog-confirm'))
    await waitFor(() => expect(tauri.clearHistory).toHaveBeenCalledTimes(1))
    expect(mockAppStore.setHistory).toHaveBeenCalledWith([])
  })

  // `waitFor` because AnimatePresence keeps the node mounted for its exit animation.
  it('deletes nothing when the confirmation is dismissed', async () => {
    render(<History />)
    fireEvent.click(screen.getByText('history.clearAll'))
    fireEvent.click(screen.getByTestId('confirm-dialog-cancel'))
    await waitFor(() => expect(screen.queryByTestId('confirm-dialog')).not.toBeInTheDocument())
    expect(tauri.clearHistory).not.toHaveBeenCalled()
  })

  it('dismisses on Escape without deleting', async () => {
    render(<History />)
    fireEvent.click(screen.getByText('history.clearAll'))
    fireEvent.keyDown(window, { key: 'Escape' })
    await waitFor(() => expect(screen.queryByTestId('confirm-dialog')).not.toBeInTheDocument())
    expect(tauri.clearHistory).not.toHaveBeenCalled()
  })
})
