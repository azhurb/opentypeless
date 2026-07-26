/**
 * Visibility logic for the two permission banners in the main window.
 *
 *  - AccessibilityBanner appears on macOS when `accessibilityTrusted=false`
 *    and disappears once the user dismisses or the flag flips back to true.
 *  - MicDeniedBanner appears on macOS for the `denied` / `restricted` mic
 *    statuses; `not_determined` and `authorized` keep it hidden.
 *
 * Both banners must stay hidden entirely on non-macOS (Linux / Windows users
 * have no per-app gate to surface).
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import React from 'react'
import { useAppStore } from '../../../stores/appStore'

afterEach(() => {
  cleanup()
})

// Lucide icons render as SVGs; nothing test-relevant. framer-motion is
// stripped (the AnimatePresence wrapper would otherwise hide content during
// exit animations and confuse synchronous getByText).
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

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: 'en', changeLanguage: vi.fn() },
  }),
}))

vi.mock('../../../lib/tauri', () => ({
  checkAccessibilityPermission: vi.fn().mockResolvedValue(true),
  requestAccessibilityPermission: vi.fn().mockResolvedValue(true),
}))

vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }))

import { AccessibilityBanner } from '../AccessibilityBanner'
import { MicDeniedBanner } from '../MicDeniedBanner'

function resetStore() {
  useAppStore.setState(useAppStore.getInitialState())
}

function mockPlatform(platform: string) {
  Object.defineProperty(window.navigator, 'platform', {
    value: platform,
    configurable: true,
  })
}

describe('AccessibilityBanner', () => {
  beforeEach(() => {
    resetStore()
    mockPlatform('MacIntel')
  })

  it('is hidden when accessibilityTrusted=true', () => {
    useAppStore.getState().setAccessibilityTrusted(true)
    render(<AccessibilityBanner />)
    expect(screen.queryByText('settings.accessibilityRequired')).not.toBeInTheDocument()
  })

  it('shows when on macOS and accessibilityTrusted=false', () => {
    useAppStore.getState().setAccessibilityTrusted(false)
    render(<AccessibilityBanner />)
    expect(screen.getByText(/settings.accessibilityRequired/)).toBeInTheDocument()
    expect(screen.getByText('settings.grantPermission')).toBeInTheDocument()
  })

  it('is hidden on non-macOS even when trust is false', () => {
    mockPlatform('Linux x86_64')
    useAppStore.getState().setAccessibilityTrusted(false)
    render(<AccessibilityBanner />)
    expect(screen.queryByText('settings.grantPermission')).not.toBeInTheDocument()
  })

  it('hides after the dismiss button is clicked', () => {
    useAppStore.getState().setAccessibilityTrusted(false)
    render(<AccessibilityBanner />)
    const dismiss = screen.getByRole('button', { name: /dismiss/i })
    fireEvent.click(dismiss)
    expect(screen.queryByText('settings.grantPermission')).not.toBeInTheDocument()
  })
})

describe('MicDeniedBanner', () => {
  beforeEach(() => {
    resetStore()
    mockPlatform('MacIntel')
  })

  it('is hidden when micAuthStatus is authorized', () => {
    useAppStore.getState().setMicAuthStatus('authorized')
    render(<MicDeniedBanner />)
    expect(screen.queryByText('permissions.microphone.denied')).not.toBeInTheDocument()
  })

  it('is hidden when micAuthStatus is not_determined (we have not yet asked)', () => {
    useAppStore.getState().setMicAuthStatus('not_determined')
    render(<MicDeniedBanner />)
    expect(screen.queryByText('permissions.microphone.denied')).not.toBeInTheDocument()
  })

  it('shows when micAuthStatus is denied', () => {
    useAppStore.getState().setMicAuthStatus('denied')
    render(<MicDeniedBanner />)
    expect(screen.getByText('permissions.microphone.denied')).toBeInTheDocument()
    expect(screen.getByText('permissions.openSettings')).toBeInTheDocument()
  })

  it('shows when micAuthStatus is restricted (parental / MDM lockdown)', () => {
    useAppStore.getState().setMicAuthStatus('restricted')
    render(<MicDeniedBanner />)
    expect(screen.getByText('permissions.microphone.denied')).toBeInTheDocument()
  })

  it('is hidden on non-macOS', () => {
    mockPlatform('Win32')
    useAppStore.getState().setMicAuthStatus('denied')
    render(<MicDeniedBanner />)
    expect(screen.queryByText('permissions.microphone.denied')).not.toBeInTheDocument()
  })

  it('hides after dismiss', () => {
    useAppStore.getState().setMicAuthStatus('denied')
    render(<MicDeniedBanner />)
    fireEvent.click(screen.getByRole('button', { name: /dismiss/i }))
    expect(screen.queryByText('permissions.microphone.denied')).not.toBeInTheDocument()
  })
})
