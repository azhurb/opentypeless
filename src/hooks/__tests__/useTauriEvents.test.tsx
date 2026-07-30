/**
 * useTauriEvents owns the contract between Rust pipeline events and the
 * Zustand store. The permission-error mapping in particular is the only thing
 * keeping the banner / capsule in sync with what actually happened in Rust —
 * if these dispatches regress, the user grants permission, dictates, paste
 * silently fails, and the UI shows nothing useful.
 *
 * Strategy: mock `listen` from @tauri-apps/api/event so we can capture each
 * registered handler and then invoke it directly. That's both faster and
 * tighter than running through the real event bus.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { renderHook, act, cleanup } from '@testing-library/react'
import { useAppStore } from '../../stores/appStore'

afterEach(() => {
  cleanup()
})

// Capture every `listen(event, handler)` call so tests can fire events by
// looking the handler up by name.
type Handler = (payload: unknown) => void
const handlers = new Map<string, Handler>()

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((event: string, handler: Handler) => {
    handlers.set(event, handler)
    return Promise.resolve(() => handlers.delete(event))
  }),
}))

// useTauriEvents calls getHistory / getDictionary on certain state transitions.
// Stub them out so we don't make accidental invoke() calls during the test.
vi.mock('../../lib/tauri', () => ({
  getHistory: vi.fn().mockResolvedValue([]),
  getDictionary: vi.fn().mockResolvedValue([]),
}))

import { useTauriEvents } from '../useTauriEvents'

function resetStore() {
  useAppStore.setState(useAppStore.getInitialState())
  handlers.clear()
}

/** Real callers receive `{ payload, … }`; the hook's wrapper reads `.payload`. */
async function fire(event: string, payload: unknown) {
  const handler = handlers.get(event)
  if (!handler) throw new Error(`No listener registered for "${event}"`)
  await act(async () => {
    handler({ event, payload, id: 0 } as unknown)
    await Promise.resolve()
  })
}

describe('useTauriEvents — pipeline:error handling', () => {
  beforeEach(() => {
    resetStore()
  })

  it('forwards generic errors verbatim into pipelineError', async () => {
    renderHook(() => useTauriEvents())
    // Wait for listen() promises to resolve and handlers to register.
    await act(async () => {
      await Promise.resolve()
    })

    await fire('pipeline:error', 'LLM polishing failed: rate limited')
    expect(useAppStore.getState().pipelineError).toBe('LLM polishing failed: rate limited')
  })

  it('flips accessibilityTrusted to false when error is ACCESSIBILITY_REQUIRED', async () => {
    // Start trusted (default). Pre-flight check fired this code from Rust.
    expect(useAppStore.getState().accessibilityTrusted).toBe(true)
    renderHook(() => useTauriEvents())
    await act(async () => {
      await Promise.resolve()
    })

    await fire('pipeline:error', 'ACCESSIBILITY_REQUIRED')
    expect(useAppStore.getState().pipelineError).toBe('ACCESSIBILITY_REQUIRED')
    expect(useAppStore.getState().accessibilityTrusted).toBe(false)
  })

  it('flips micAuthStatus to denied when error is MICROPHONE_DENIED', async () => {
    expect(useAppStore.getState().micAuthStatus).toBe('authorized')
    renderHook(() => useTauriEvents())
    await act(async () => {
      await Promise.resolve()
    })

    await fire('pipeline:error', 'MICROPHONE_DENIED')
    expect(useAppStore.getState().pipelineError).toBe('MICROPHONE_DENIED')
    expect(useAppStore.getState().micAuthStatus).toBe('denied')
  })

  it('does not touch permission flags on unrelated errors', async () => {
    useAppStore.getState().setAccessibilityTrusted(true)
    useAppStore.getState().setMicAuthStatus('authorized')
    renderHook(() => useTauriEvents())
    await act(async () => {
      await Promise.resolve()
    })

    await fire('pipeline:error', 'No speech detected. Please try again.')
    expect(useAppStore.getState().accessibilityTrusted).toBe(true)
    expect(useAppStore.getState().micAuthStatus).toBe('authorized')
  })
})

describe('useTauriEvents — permissions:mic_status event', () => {
  beforeEach(() => {
    resetStore()
  })

  it('updates micAuthStatus when Rust emits a fresh status snapshot', async () => {
    renderHook(() => useTauriEvents())
    await act(async () => {
      await Promise.resolve()
    })

    await fire('permissions:mic_status', 'denied')
    expect(useAppStore.getState().micAuthStatus).toBe('denied')

    await fire('permissions:mic_status', 'authorized')
    expect(useAppStore.getState().micAuthStatus).toBe('authorized')
  })
})

describe('useTauriEvents — config:changed event', () => {
  // Each Tauri window owns its own webview and its own Zustand instance, so the
  // capsule window can't see config edits saved from the main Settings pane
  // without a cross-window notification. Rust emits `config:changed` after
  // persisting; this listener is what keeps the capsule's `capsule_auto_hide`
  // (and other config-derived UI like the duration timer) in sync without an
  // app restart.

  beforeEach(() => {
    resetStore()
  })

  it('replaces the store config with the event payload', async () => {
    renderHook(() => useTauriEvents())
    await act(async () => {
      await Promise.resolve()
    })

    // Sanity: default has auto_hide off.
    expect(useAppStore.getState().config.capsule_auto_hide).toBe(false)

    const updated = {
      ...useAppStore.getState().config,
      capsule_auto_hide: true,
      max_recording_seconds: 90,
    }
    await fire('config:changed', updated)

    expect(useAppStore.getState().config.capsule_auto_hide).toBe(true)
    expect(useAppStore.getState().config.max_recording_seconds).toBe(90)
    expect(useAppStore.getState().configLoaded).toBe(true)
  })
})

describe('useTauriEvents — selected-text editing events', () => {
  beforeEach(() => {
    resetStore()
  })

  async function mount() {
    renderHook(() => useTauriEvents())
    await act(async () => {
      await Promise.resolve()
    })
  }

  it('sets editingSelection when Rust reports a captured selection', async () => {
    await mount()
    await fire('pipeline:editing_selection', true)
    expect(useAppStore.getState().editingSelection).toBe(true)
  })

  it('clears editingSelection on a false payload', async () => {
    // Rust emits this on every run, `false` included. Honouring the false is what
    // stops a run with nothing selected from inheriting the previous run's ring —
    // relying on the idle transition alone would leave a window where the ring is
    // up for a dictation that isn't editing anything.
    await mount()
    await fire('pipeline:editing_selection', true)
    await fire('pipeline:editing_selection', false)
    expect(useAppStore.getState().editingSelection).toBe(false)
  })

  it('clears editingSelection when the pipeline returns to idle', async () => {
    await mount()
    await fire('pipeline:editing_selection', true)
    await fire('pipeline:state', 'idle')
    expect(useAppStore.getState().editingSelection).toBe(false)
  })

  it('raises the edited tip on output:edited', async () => {
    await mount()
    await fire('output:edited', undefined)
    expect(useAppStore.getState().editedTip).toBe(true)
  })

  it('dismisses a lingering edited tip when a new recording starts', async () => {
    await mount()
    await fire('output:edited', undefined)
    await fire('pipeline:state', 'recording')
    expect(useAppStore.getState().editedTip).toBe(false)
    // The clipboard tip is cleared by the same branch and must stay cleared.
    expect(useAppStore.getState().clipboardTip).toBe(false)
  })
})
