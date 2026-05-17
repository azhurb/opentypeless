import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { renderHook } from '@testing-library/react'

const listeners: Record<string, ((e: { payload: unknown }) => void)[]> = {}

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((event: string, handler: (e: { payload: unknown }) => void) => {
    if (!listeners[event]) listeners[event] = []
    listeners[event].push(handler)
    return Promise.resolve(() => {
      const idx = listeners[event].indexOf(handler)
      if (idx >= 0) listeners[event].splice(idx, 1)
    })
  }),
}))

const showMock = vi.fn()
vi.mock('../../components/Toast', () => ({
  toast: Object.assign((msg: string) => showMock(msg), {
    success: (msg: string) => showMock(msg),
    error: (msg: string) => showMock(msg),
    info: (msg: string) => showMock(msg),
  }),
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (_key: string, vars?: Record<string, string>) =>
      `Detected ${vars?.language ?? ''} — add it in Settings?`,
  }),
}))

let mockSttLanguages: string[] = []
vi.mock('../../stores/appStore', () => ({
  useAppStore: Object.assign(
    (selector: any) => selector({ config: { stt_languages: mockSttLanguages } }),
    {
      getState: () => ({ config: { stt_languages: mockSttLanguages } }),
    },
  ),
}))

import { useDetectedLanguageNotifier } from '../useDetectedLanguageNotifier'
import { _resetRateLimit } from '../useRateLimitedToast'

async function flush() {
  await new Promise((r) => setTimeout(r, 0))
}

function emit(event: string, payload: unknown) {
  for (const h of listeners[event] ?? []) h({ payload })
}

describe('useDetectedLanguageNotifier', () => {
  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    showMock.mockReset()
    _resetRateLimit()
    for (const k of Object.keys(listeners)) listeners[k] = []
    mockSttLanguages = []
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('does not fire when stt_languages is empty (auto-detect mode)', async () => {
    mockSttLanguages = []
    renderHook(() => useDetectedLanguageNotifier())
    await flush()
    emit('pipeline:timing', { detected_language: 'de' })
    expect(showMock).not.toHaveBeenCalled()
  })

  it('does not fire when the detected language is already in the user set', async () => {
    mockSttLanguages = ['en', 'de']
    renderHook(() => useDetectedLanguageNotifier())
    await flush()
    emit('pipeline:timing', { detected_language: 'de' })
    expect(showMock).not.toHaveBeenCalled()
  })

  it('does not fire when the payload has no detected_language', async () => {
    mockSttLanguages = ['en']
    renderHook(() => useDetectedLanguageNotifier())
    await flush()
    emit('pipeline:timing', { detected_language: null })
    expect(showMock).not.toHaveBeenCalled()
  })

  it('fires when a language outside the user set is detected', async () => {
    mockSttLanguages = ['en']
    renderHook(() => useDetectedLanguageNotifier())
    await flush()
    emit('pipeline:timing', { detected_language: 'de' })
    expect(showMock).toHaveBeenCalledTimes(1)
    expect(showMock.mock.calls[0][0]).toMatch(/de/i)
  })

  it('respects the 10s rate limit between consecutive mismatches', async () => {
    mockSttLanguages = ['en']
    renderHook(() => useDetectedLanguageNotifier())
    await flush()
    emit('pipeline:timing', { detected_language: 'de' })
    emit('pipeline:timing', { detected_language: 'de' })
    expect(showMock).toHaveBeenCalledTimes(1)
  })
})
