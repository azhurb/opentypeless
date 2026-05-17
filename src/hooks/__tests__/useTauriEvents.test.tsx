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
