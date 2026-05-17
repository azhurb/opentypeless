import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'
import { History } from '../index'
import type { HistoryEntry } from '../../../stores/appStore'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}))

vi.mock('../../../lib/tauri')

const mockAppStore: { history: HistoryEntry[]; setHistory: (h: HistoryEntry[]) => void } = {
  history: [],
  setHistory: vi.fn(),
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
