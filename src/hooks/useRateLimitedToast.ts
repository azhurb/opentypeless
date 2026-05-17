import { useCallback } from 'react'
import { toast } from '../components/Toast'

const lastShown = new Map<string, number>()

const DEFAULT_COOLDOWN_MS = 10_000

export function useRateLimitedToast(cooldownMs: number = DEFAULT_COOLDOWN_MS) {
  return useCallback(
    (key: string, message: string) => {
      const now = Date.now()
      const prev = lastShown.get(key)
      if (prev !== undefined && now - prev < cooldownMs) {
        return
      }
      lastShown.set(key, now)
      toast(message)
    },
    [cooldownMs],
  )
}

/** Test-only: reset the cooldown map between cases. */
export function _resetRateLimit() {
  lastShown.clear()
}
