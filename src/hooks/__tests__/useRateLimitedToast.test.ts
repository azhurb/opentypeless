import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { renderHook } from '@testing-library/react'
import { useRateLimitedToast, _resetRateLimit } from '../useRateLimitedToast'

const showMock = vi.fn()

vi.mock('../../components/Toast', () => ({
  toast: Object.assign((msg: string) => showMock(msg), {
    success: (msg: string) => showMock(msg),
    error: (msg: string) => showMock(msg),
    info: (msg: string) => showMock(msg),
  }),
}))

describe('useRateLimitedToast', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    showMock.mockReset()
    _resetRateLimit()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('shows the toast on first call', () => {
    const { result } = renderHook(() => useRateLimitedToast())
    result.current('detected-language', 'Detected German')
    expect(showMock).toHaveBeenCalledWith('Detected German')
    expect(showMock).toHaveBeenCalledTimes(1)
  })

  it('suppresses a second call to the same key within the cooldown window', () => {
    const { result } = renderHook(() => useRateLimitedToast(10_000))
    result.current('detected-language', 'first')
    result.current('detected-language', 'second')
    expect(showMock).toHaveBeenCalledTimes(1)
    expect(showMock).toHaveBeenCalledWith('first')
  })

  it('allows a second call once the cooldown elapses', () => {
    const { result } = renderHook(() => useRateLimitedToast(10_000))
    result.current('detected-language', 'first')
    vi.advanceTimersByTime(10_001)
    result.current('detected-language', 'second')
    expect(showMock).toHaveBeenCalledTimes(2)
    expect(showMock).toHaveBeenNthCalledWith(2, 'second')
  })

  it('tracks cooldowns per key independently', () => {
    const { result } = renderHook(() => useRateLimitedToast(10_000))
    result.current('key-a', 'a1')
    result.current('key-b', 'b1')
    expect(showMock).toHaveBeenCalledTimes(2)
    result.current('key-a', 'a2')
    expect(showMock).toHaveBeenCalledTimes(2)
  })
})
