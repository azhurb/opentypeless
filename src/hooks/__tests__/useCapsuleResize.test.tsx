/**
 * useCapsuleResize is the only thing that converts `capsule_auto_hide` into
 * an actual show()/hide() call on the capsule's NSWindow / HWND. When the
 * Settings pane in the main window saves a change, Rust re-broadcasts the
 * new config via `config:changed`; the capsule window's useTauriEvents
 * dispatches setConfig, and *this* hook is supposed to react. If the
 * subscription path through Zustand breaks, the bug we just fixed comes
 * back: toggling "Hide capsule when idle" in Settings does nothing until
 * the app is relaunched.
 *
 * Strategy: mock the dynamic `@tauri-apps/api/window` import so we can
 * observe show()/hide() calls without a real window. Drive the store the
 * same way useTauriEvents would after a config:changed event.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { renderHook, act, cleanup } from '@testing-library/react'
import { useAppStore, type AppConfig } from '../../stores/appStore'

afterEach(() => {
  cleanup()
})

const show = vi.fn().mockResolvedValue(undefined)
const hide = vi.fn().mockResolvedValue(undefined)
const setSize = vi.fn().mockResolvedValue(undefined)
const setPosition = vi.fn().mockResolvedValue(undefined)
const outerPosition = vi.fn().mockResolvedValue({ x: 0, y: 0 })

vi.mock('@tauri-apps/api/window', () => {
  class LogicalSize {
    constructor(
      public width: number,
      public height: number,
    ) {}
  }
  class LogicalPosition {
    constructor(
      public x: number,
      public y: number,
    ) {}
  }
  return {
    getCurrentWindow: () => ({
      setSize,
      setPosition,
      show,
      hide,
      outerPosition,
    }),
    LogicalSize,
    LogicalPosition,
    // Stubbed monitor calls — placeBottomCenterOfActiveMonitor just bails
    // out silently when these return null, which is fine for our assertions.
    currentMonitor: vi.fn().mockResolvedValue({
      size: { width: 1920, height: 1080 },
      position: { x: 0, y: 0 },
      scaleFactor: 1,
    }),
    monitorFromPoint: vi.fn().mockResolvedValue(null),
    primaryMonitor: vi.fn().mockResolvedValue({
      size: { width: 1920, height: 1080 },
      position: { x: 0, y: 0 },
      scaleFactor: 1,
    }),
    cursorPosition: vi.fn().mockResolvedValue({ x: 0, y: 0 }),
  }
})

import { useCapsuleResize } from '../useCapsuleResize'

function makeConfig(overrides: Partial<AppConfig> = {}): AppConfig {
  return {
    ...useAppStore.getInitialState().config,
    ...overrides,
  }
}

async function flushAsync() {
  // useCapsuleResize awaits a dynamic import, then chains several `await`s
  // inside the .then() callback. We need multiple microtask flushes for
  // all of them to resolve before our assertions run.
  for (let i = 0; i < 10; i++) {
    await act(async () => {
      await Promise.resolve()
    })
  }
}

describe('useCapsuleResize — reacts to config changes from other windows', () => {
  beforeEach(() => {
    useAppStore.setState(useAppStore.getInitialState())
    show.mockClear()
    hide.mockClear()
    setSize.mockClear()
    setPosition.mockClear()
  })

  it('hides the capsule when capsule_auto_hide flips to true while idle', async () => {
    // Start: auto-hide off, idle. Config loaded (mimics getConfig completing).
    useAppStore.getState().setConfig(makeConfig({ capsule_auto_hide: false }))

    renderHook(() => useCapsuleResize())
    await flushAsync()

    // First mount with shouldBeVisible=true should have called show(), not hide().
    expect(show).toHaveBeenCalledTimes(1)
    expect(hide).not.toHaveBeenCalled()

    show.mockClear()

    // Now simulate the main window saving "Hide capsule when idle = true".
    // useTauriEvents dispatches setConfig with the broadcast payload.
    act(() => {
      useAppStore.getState().setConfig(makeConfig({ capsule_auto_hide: true }))
    })
    await flushAsync()

    expect(hide).toHaveBeenCalledTimes(1)
    expect(show).not.toHaveBeenCalled()
  })

  it('shows the capsule when capsule_auto_hide flips back to false while idle', async () => {
    useAppStore.getState().setConfig(makeConfig({ capsule_auto_hide: true }))

    renderHook(() => useCapsuleResize())
    await flushAsync()

    // Initial mount with auto-hide on + idle → window stays hidden.
    expect(hide).toHaveBeenCalledTimes(1)
    expect(show).not.toHaveBeenCalled()

    hide.mockClear()

    act(() => {
      useAppStore.getState().setConfig(makeConfig({ capsule_auto_hide: false }))
    })
    await flushAsync()

    expect(show).toHaveBeenCalledTimes(1)
    expect(hide).not.toHaveBeenCalled()
  })
})
