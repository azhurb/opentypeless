import { useEffect } from 'react'
import { listen } from '@tauri-apps/api/event'
import { useTranslation } from 'react-i18next'
import { useAppStore } from '../stores/appStore'
import { useRateLimitedToast } from './useRateLimitedToast'
import { LANGUAGES } from '../lib/constants'

interface TimingPayload {
  detected_language?: string | null
}

function displayName(code: string): string {
  const entry = LANGUAGES.find((l) => l.value === code)
  return entry?.label ?? code.toUpperCase()
}

/**
 * Listens for `pipeline:timing` and fires a rate-limited toast when the STT
 * detects a language the user hasn't configured. No toast in auto-detect mode
 * (empty `stt_languages`) — there's nothing to compare against.
 */
export function useDetectedLanguageNotifier() {
  const showToast = useRateLimitedToast(10_000)
  const { t } = useTranslation()

  useEffect(() => {
    let cancelled = false
    let unlisten: (() => void) | undefined

    listen<TimingPayload>('pipeline:timing', (e) => {
      const detected = e.payload?.detected_language
      if (!detected) return
      const userLangs = useAppStore.getState().config.stt_languages
      if (userLangs.length === 0) return
      if (userLangs.includes(detected)) return
      showToast(
        `detected-language:${detected}`,
        t('notifications.detectedLanguage', { language: displayName(detected) }),
      )
    })
      .then((u) => {
        if (cancelled) u()
        else unlisten = u
      })
      .catch((err) => {
        console.error('Failed to register pipeline:timing listener:', err)
      })

    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [showToast, t])
}
