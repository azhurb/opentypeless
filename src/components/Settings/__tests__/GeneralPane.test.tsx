import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/react'
import { GeneralPane } from '../GeneralPane'

// Mock Tauri — GeneralPane's hotkey recorder reaches for it on mount.
vi.mock('../../../lib/tauri')

// Mock i18n — interpolation is exercised here because the retention labels
// depend on it, so `t` must honour the `{{days}}` placeholder.
vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    // `opts` is an interpolation object for the retention labels, but GeneralPane
    // also calls `t(key, 'Default string')` elsewhere — handle both shapes.
    t: (key: string, opts?: Record<string, unknown> | string) => {
      const translations: Record<string, string> = {
        'settings.history': 'History',
        'settings.saveHistory': 'Save dictation history',
        'settings.keepHistoryFor': 'Keep history for',
        'settings.retentionForever': 'Forever',
        'settings.retentionDays': '{{days}} days',
        'settings.retentionHintForever': 'Entries are kept until you delete them.',
        'settings.retentionHintDays': 'Entries older than {{days}} days are deleted.',
        'settings.retentionConfirm': 'Keep history for {{days}} days?',
        'common.delete': 'Delete',
        'common.cancel': 'Cancel',
      }
      if (typeof opts === 'string') return translations[key] || opts
      const value = translations[key] || key
      return opts && 'days' in opts ? value.replace('{{days}}', String(opts.days)) : value
    },
  }),
}))

const mockAppStore = {
  config: {
    hotkey: 'Alt+/',
    hotkey_mode: 'hold' as string,
    max_recording_seconds: 30,
    auto_start: false,
    capsule_auto_hide: false,
    history_enabled: true,
    history_retention_days: 0,
  },
  savedConfig: null as { history_retention_days: number } | null,
  updateConfig: vi.fn(),
}

vi.mock('../../../stores/appStore', () => ({
  useAppStore: (selector: any) => {
    if (typeof selector === 'function') {
      return selector(mockAppStore)
    }
    return mockAppStore
  },
}))

function retentionSelect() {
  return screen.getByLabelText('Keep history for') as HTMLSelectElement
}

describe('GeneralPane history controls', () => {
  beforeEach(() => {
    mockAppStore.config.history_enabled = true
    mockAppStore.config.history_retention_days = 0
    mockAppStore.savedConfig = { history_retention_days: 0 }
    vi.clearAllMocks()
  })

  afterEach(() => {
    cleanup()
  })

  it('renders the toggle and the retention options', () => {
    render(<GeneralPane />)
    expect(screen.getByText('Save dictation history')).toBeInTheDocument()
    const options = Array.from(retentionSelect().options).map((o) => o.textContent)
    expect(options).toEqual(['Forever', '7 days', '30 days', '90 days'])
  })

  it('defaults the retention select to Forever', () => {
    render(<GeneralPane />)
    expect(retentionSelect().value).toBe('0')
  })

  it('pushes history_enabled through updateConfig when toggled off', () => {
    render(<GeneralPane />)
    fireEvent.click(screen.getByText('Save dictation history'))
    expect(mockAppStore.updateConfig).toHaveBeenCalledWith({ history_enabled: false })
  })

  it('pushes history_retention_days as a number, not a string', () => {
    render(<GeneralPane />)
    // Forever -> 30 narrows, so it routes through the confirmation.
    fireEvent.change(retentionSelect(), { target: { value: '30' } })
    fireEvent.click(screen.getByTestId('confirm-dialog-confirm'))
    expect(mockAppStore.updateConfig).toHaveBeenCalledWith({ history_retention_days: 30 })
  })

  it('keeps the retention select usable while saving is off, because it still prunes', () => {
    mockAppStore.config.history_enabled = false
    render(<GeneralPane />)
    expect(retentionSelect()).not.toBeDisabled()
  })

  it('shows the no-deletion hint when set to Forever', () => {
    render(<GeneralPane />)
    expect(screen.getByText('Entries are kept until you delete them.')).toBeInTheDocument()
  })

  it('shows a hint naming the window when a limit is set', () => {
    mockAppStore.config.history_retention_days = 30
    mockAppStore.savedConfig = { history_retention_days: 30 }
    render(<GeneralPane />)
    expect(screen.getByText('Entries older than 30 days are deleted.')).toBeInTheDocument()
  })

  it('renders an out-of-range persisted value instead of silently showing Forever', () => {
    mockAppStore.config.history_retention_days = 14
    mockAppStore.savedConfig = { history_retention_days: 14 }
    render(<GeneralPane />)
    expect(retentionSelect().value).toBe('14')
    expect(Array.from(retentionSelect().options).map((o) => o.textContent)).toEqual([
      'Forever',
      '7 days',
      '14 days',
      '30 days',
      '90 days',
    ])
  })

  // These drive the real ConfirmDialog. They deliberately do not stub
  // `window.confirm`: it returns falsy without displaying anything on macOS
  // (wry implements no WKWebView JS-dialog delegate), so a mock would assert a
  // contract the platform does not provide and hide the bug entirely.
  describe('confirmation before a narrowing change', () => {
    it('asks before narrowing from Forever, and does not apply it yet', () => {
      render(<GeneralPane />)
      fireEvent.change(retentionSelect(), { target: { value: '7' } })
      expect(screen.getByTestId('confirm-dialog')).toBeInTheDocument()
      expect(screen.getByText('Keep history for 7 days?')).toBeInTheDocument()
      expect(mockAppStore.updateConfig).not.toHaveBeenCalled()
    })

    // `waitFor` because AnimatePresence keeps the node mounted for its exit animation.
    it('applies the narrower window once confirmed', async () => {
      render(<GeneralPane />)
      fireEvent.change(retentionSelect(), { target: { value: '7' } })
      fireEvent.click(screen.getByTestId('confirm-dialog-confirm'))
      expect(mockAppStore.updateConfig).toHaveBeenCalledWith({ history_retention_days: 7 })
      await waitFor(() => expect(screen.queryByTestId('confirm-dialog')).not.toBeInTheDocument())
    })

    it('discards the change when the confirmation is dismissed', async () => {
      render(<GeneralPane />)
      fireEvent.change(retentionSelect(), { target: { value: '7' } })
      fireEvent.click(screen.getByTestId('confirm-dialog-cancel'))
      expect(mockAppStore.updateConfig).not.toHaveBeenCalled()
      await waitFor(() => expect(screen.queryByTestId('confirm-dialog')).not.toBeInTheDocument())
    })

    it('does not ask when widening the window — nothing is deleted', () => {
      mockAppStore.config.history_retention_days = 7
      mockAppStore.savedConfig = { history_retention_days: 7 }
      render(<GeneralPane />)
      fireEvent.change(retentionSelect(), { target: { value: '90' } })
      expect(screen.queryByTestId('confirm-dialog')).not.toBeInTheDocument()
      expect(mockAppStore.updateConfig).toHaveBeenCalledWith({ history_retention_days: 90 })
    })

    it('does not ask when switching to Forever', () => {
      mockAppStore.config.history_retention_days = 7
      mockAppStore.savedConfig = { history_retention_days: 7 }
      render(<GeneralPane />)
      fireEvent.change(retentionSelect(), { target: { value: '0' } })
      expect(screen.queryByTestId('confirm-dialog')).not.toBeInTheDocument()
      expect(mockAppStore.updateConfig).toHaveBeenCalledWith({ history_retention_days: 0 })
    })
  })
})
