/**
 * CapsuleError covers the user-facing rendering of pipeline errors:
 *   - permission codes render localized copy, not the bare error string
 *   - permission codes are sticky (no 2.5s auto-clear); other errors auto-clear
 *
 * The 2.5s sticky test is what guards against regressions of the "user sees
 * MICROPHONE_DENIED for 2.5s then it vanishes" complaint that originally
 * surfaced during manual testing — the capsule needs to stay long enough to
 * tap.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, act, cleanup } from '@testing-library/react'
import React from 'react'
import { useAppStore } from '../../../stores/appStore'

afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

// framer-motion noise stripper (mirrors Settings.test.tsx)
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

import { CapsuleError } from '../CapsuleError'

function resetStore() {
  useAppStore.setState(useAppStore.getInitialState())
}

describe('CapsuleError message rendering', () => {
  beforeEach(() => {
    resetStore()
  })

  it('renders ACCESSIBILITY_REQUIRED via the localized capsule key', () => {
    useAppStore.getState().setPipelineError('ACCESSIBILITY_REQUIRED')
    render(<CapsuleError />)
    expect(screen.getByText('capsule.accessibilityRequired')).toBeInTheDocument()
    expect(screen.queryByText('ACCESSIBILITY_REQUIRED')).not.toBeInTheDocument()
  })

  it('renders MICROPHONE_DENIED via the localized capsule key', () => {
    useAppStore.getState().setPipelineError('MICROPHONE_DENIED')
    render(<CapsuleError />)
    expect(screen.getByText('capsule.microphoneDenied')).toBeInTheDocument()
    expect(screen.queryByText('MICROPHONE_DENIED')).not.toBeInTheDocument()
  })

  it('renders unknown errors verbatim', () => {
    useAppStore.getState().setPipelineError('LLM polishing failed: 429')
    render(<CapsuleError />)
    expect(screen.getByText('LLM polishing failed: 429')).toBeInTheDocument()
  })

  it('falls back to "An error occurred" when pipelineError is null', () => {
    // pipelineError is null (default) but CapsuleError is rendered anyway
    // (defensive — in practice the parent gates the render).
    render(<CapsuleError />)
    expect(screen.getByText('An error occurred')).toBeInTheDocument()
  })
})

describe('CapsuleError sticky vs auto-clear', () => {
  beforeEach(() => {
    resetStore()
    vi.useFakeTimers()
  })

  it('auto-clears non-permission errors after 2.5s', () => {
    useAppStore.getState().setPipelineError('LLM polishing failed: 429')
    render(<CapsuleError />)
    expect(useAppStore.getState().pipelineError).toBe('LLM polishing failed: 429')

    act(() => {
      vi.advanceTimersByTime(2500)
    })
    expect(useAppStore.getState().pipelineError).toBeNull()
  })

  it('does NOT auto-clear ACCESSIBILITY_REQUIRED', () => {
    useAppStore.getState().setPipelineError('ACCESSIBILITY_REQUIRED')
    render(<CapsuleError />)

    act(() => {
      vi.advanceTimersByTime(5000)
    })
    expect(useAppStore.getState().pipelineError).toBe('ACCESSIBILITY_REQUIRED')
  })

  it('does NOT auto-clear MICROPHONE_DENIED', () => {
    useAppStore.getState().setPipelineError('MICROPHONE_DENIED')
    render(<CapsuleError />)

    act(() => {
      vi.advanceTimersByTime(5000)
    })
    expect(useAppStore.getState().pipelineError).toBe('MICROPHONE_DENIED')
  })
})
