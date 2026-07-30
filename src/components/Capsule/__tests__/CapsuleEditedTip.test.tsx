/**
 * CapsuleEditedTip covers the "your selection was replaced" receipt:
 *   - renders the localized copy with the platform undo shortcut
 *   - auto-dismisses after ~3s by clearing the editedTip store flag
 *
 * The auto-dismiss test guards the same regression as the clipboard tip's: the
 * pill must clear itself so it never sticks around blocking the idle capsule.
 * The 3s figure is load-bearing — long enough to read, short enough that the
 * capsule is back to idle before the user's next dictation.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, act, cleanup } from '@testing-library/react'
import React from 'react'
import { useAppStore } from '../../../stores/appStore'

afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

// framer-motion noise stripper (mirrors CapsuleClipboardTip.test.tsx)
const MOTION_PROPS = new Set(['initial', 'animate', 'exit', 'transition', 'whileHover', 'whileTap'])
vi.mock('framer-motion', () => ({
  AnimatePresence: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  motion: new Proxy(
    {},
    {
      get:
        (_t, tag: string) =>
        ({ children, ...rest }: Record<string, unknown> & { children?: React.ReactNode }) => {
          const domProps: Record<string, unknown> = {}
          for (const [k, v] of Object.entries(rest)) {
            if (!MOTION_PROPS.has(k)) domProps[k] = v
          }
          return React.createElement(tag as string, domProps, children)
        },
    },
  ),
}))

// Interpolate the shortcut so the platform-aware value is assertable.
vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, params?: Record<string, string>) =>
      params?.shortcut ? `${key}:${params.shortcut}` : key,
    i18n: { language: 'en', changeLanguage: vi.fn() },
  }),
}))

import { CapsuleEditedTip } from '../CapsuleEditedTip'

describe('CapsuleEditedTip', () => {
  beforeEach(() => {
    useAppStore.setState(useAppStore.getInitialState())
  })

  it('renders the localized tip with the undo shortcut', () => {
    render(<CapsuleEditedTip />)
    // jsdom reports a non-Mac platform, so the cross-platform branch is what runs
    // here. Either value proves the shortcut was interpolated rather than dropped.
    expect(screen.getByText(/^capsule\.editedTip:(⌘Z|Ctrl\+Z)$/)).toBeInTheDocument()
  })

  it('auto-dismisses after 3s by clearing editedTip', () => {
    vi.useFakeTimers()
    useAppStore.getState().setEditedTip(true)
    render(<CapsuleEditedTip />)
    expect(useAppStore.getState().editedTip).toBe(true)

    act(() => {
      vi.advanceTimersByTime(2999)
    })
    expect(useAppStore.getState().editedTip).toBe(true)

    act(() => {
      vi.advanceTimersByTime(1)
    })
    expect(useAppStore.getState().editedTip).toBe(false)
  })
})
