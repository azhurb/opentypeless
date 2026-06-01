/**
 * CapsuleClipboardTip covers the "your dictation had nowhere to paste — it's on
 * the clipboard" hint:
 *   - renders the localized tip copy
 *   - auto-dismisses after ~4s by clearing the clipboardTip store flag
 *
 * The auto-dismiss test guards the "tip lingers forever" regression: the pill
 * must clear itself so it never sticks around blocking the idle capsule.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, act, cleanup } from '@testing-library/react'
import React from 'react'
import { useAppStore } from '../../../stores/appStore'

afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

// framer-motion noise stripper (mirrors CapsuleError.test.tsx)
const MOTION_PROPS = new Set(['initial', 'animate', 'exit', 'transition', 'whileHover', 'whileTap'])
vi.mock('framer-motion', () => ({
  AnimatePresence: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  motion: new Proxy(
    {},
    {
      get:
        (_t, tag: string) =>
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        ({ children, ...rest }: any) => {
          const domProps: Record<string, unknown> = {}
          for (const [k, v] of Object.entries(rest)) {
            if (!MOTION_PROPS.has(k)) domProps[k] = v
          }
          return React.createElement(tag as string, domProps, children)
        },
    },
  ),
}))

// Return the i18n key as the rendered string so assertions can be specific.
vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: 'en', changeLanguage: vi.fn() },
  }),
}))

import { CapsuleClipboardTip } from '../CapsuleClipboardTip'

function resetStore() {
  useAppStore.setState(useAppStore.getInitialState())
}

describe('CapsuleClipboardTip', () => {
  beforeEach(() => {
    resetStore()
  })

  it('renders the localized clipboard tip', () => {
    render(<CapsuleClipboardTip />)
    expect(screen.getByText('capsule.clipboardTip')).toBeInTheDocument()
  })

  it('auto-dismisses after 4s by clearing clipboardTip', () => {
    vi.useFakeTimers()
    useAppStore.getState().setClipboardTip(true)
    render(<CapsuleClipboardTip />)
    expect(useAppStore.getState().clipboardTip).toBe(true)

    act(() => {
      vi.advanceTimersByTime(4000)
    })
    expect(useAppStore.getState().clipboardTip).toBe(false)
  })
})
