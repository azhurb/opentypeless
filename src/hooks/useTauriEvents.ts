import { useEffect } from 'react'
import { listen } from '@tauri-apps/api/event'
import { useAppStore } from '../stores/appStore'
import type { AppConfig, PipelineState } from '../stores/appStore'
import { getHistory, getDictionary, type MicAuthStatus } from '../lib/tauri'

export function useTauriEvents() {
  const {
    setAudioVolume,
    setPartialTranscript,
    setFinalTranscript,
    appendPolishedChunk,
    setPipelineState,
    setTargetApp,
    setPipelineError,
    setConfig,
    setHistory,
    setDictionary,
    setCorrectionSuggestion,
    setAccessibilityTrusted,
    setMicAuthStatus,
    setClipboardTip,
  } = useAppStore()

  useEffect(() => {
    let cancelled = false
    const unlisteners: Array<() => void> = []

    function addListener<T>(event: string, handler: (payload: T) => void) {
      listen<T>(event, (e) => handler(e.payload))
        .then((unlisten) => {
          if (cancelled) {
            unlisten()
          } else {
            unlisteners.push(unlisten)
          }
        })
        .catch((err) => {
          console.error(`Failed to register listener for "${event}":`, err)
        })
    }

    addListener<number>('audio:volume', setAudioVolume)
    addListener<string>('stt:partial', setPartialTranscript)
    addListener<string>('stt:final', setFinalTranscript)
    addListener<string>('llm:chunk', appendPolishedChunk)
    addListener<PipelineState>('pipeline:state', (state) => {
      setPipelineState(state)
      if (state === 'recording') {
        // Clear any previous error when starting a new pipeline run
        setPipelineError(null)
        // Dismiss a lingering clipboard tip — the user is dictating again.
        setClipboardTip(false)
      }
      if (state === 'idle') {
        // Don't clear pipelineError here — CapsuleError auto-resets after 2.5s.
        // Clearing here would swallow errors from failed start() calls that
        // transition Recording → Idle in rapid succession.
        getHistory(200, 0)
          .then(setHistory)
          .catch((err) => {
            console.error('Failed to refresh history:', err)
          })
      }
    })
    addListener<string>('pipeline:target_app', setTargetApp)
    addListener<string>('pipeline:error', (error) => {
      setPipelineError(error)
      if (error === 'ACCESSIBILITY_REQUIRED') {
        // Paste was skipped because AX wasn't granted. Flip the store flag so
        // the banner / settings UI reflect reality without waiting for the
        // next focus-recheck.
        setAccessibilityTrusted(false)
      }
      if (error === 'MICROPHONE_DENIED') {
        // Pipeline refused to start because Mic is denied/restricted.
        setMicAuthStatus('denied')
      }
    })
    addListener<MicAuthStatus>('permissions:mic_status', setMicAuthStatus)

    // Dictation finished but nothing was focused to paste into — the text was
    // left on the clipboard. Surface the manual-paste tip, and clear any soft
    // polish/STT error so it doesn't mask (or re-appear after) the tip. A
    // permission error never reaches this path — it bails before output — so
    // it's safe to clear; we guard against it anyway.
    addListener<void>('output:no_target', () => {
      setClipboardTip(true)
      const err = useAppStore.getState().pipelineError
      if (err && err !== 'ACCESSIBILITY_REQUIRED' && err !== 'MICROPHONE_DENIED') {
        setPipelineError(null)
      }
    })

    addListener<void>('tray:settings', () => {
      window.location.hash = '#/settings'
    })
    addListener<void>('tray:history', () => {
      window.location.hash = '#/history'
    })
    addListener<string>('navigate', (hash) => {
      window.location.hash = hash
    })
    addListener<void>('tray:about', () => {
      window.location.hash = '#/settings'
    })

    addListener<{ rowId: number; old: string; new: string; autoConfirmMs: number }>(
      'correction:suggest',
      (payload) => {
        setCorrectionSuggestion({
          rowId: payload.rowId,
          old: payload.old,
          new: payload.new,
          autoConfirmMs: payload.autoConfirmMs,
        })
      },
    )

    addListener<void>('dictionary:changed', () => {
      getDictionary()
        .then(setDictionary)
        .catch((err) => {
          console.error('Failed to refresh dictionary:', err)
        })
    })

    // Rust broadcasts the full AppConfig after every persisted update so
    // every webview (main Settings pane, capsule) can replace its local
    // Zustand copy. The capsule is the load-bearing consumer — its
    // show/hide is derived from `config.capsule_auto_hide`, so without
    // this dispatch the "Hide capsule when idle" toggle wouldn't take
    // effect until the next app launch.
    addListener<AppConfig>('config:changed', setConfig)

    return () => {
      cancelled = true
      unlisteners.forEach((unlisten) => unlisten())
    }
  }, [
    setAudioVolume,
    setPartialTranscript,
    setFinalTranscript,
    appendPolishedChunk,
    setPipelineState,
    setTargetApp,
    setPipelineError,
    setConfig,
    setHistory,
    setDictionary,
    setCorrectionSuggestion,
    setAccessibilityTrusted,
    setMicAuthStatus,
    setClipboardTip,
  ])
}
