/**
 * PermissionsStep is the macOS onboarding card pair. Cover:
 *   - mic button calls `requestMicrophonePermission` for `not_determined`
 *   - mic button opens System Settings for `denied` (the dialog is one-shot)
 *   - mic button is hidden once granted
 *   - AX grant button calls `requestAccessibilityPermission`
 *   - non-macOS renders nothing actionable (the wrapping flow auto-advances)
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { useAppStore } from '../../../stores/appStore'

afterEach(() => {
  cleanup()
})

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: 'en', changeLanguage: vi.fn() },
  }),
}))

const checkMic = vi.fn()
const requestMic = vi.fn()
const checkAx = vi.fn()
const requestAx = vi.fn()
vi.mock('../../../lib/tauri', () => ({
  checkMicrophonePermission: (...a: unknown[]) => checkMic(...a),
  requestMicrophonePermission: (...a: unknown[]) => requestMic(...a),
  checkAccessibilityPermission: (...a: unknown[]) => checkAx(...a),
  requestAccessibilityPermission: (...a: unknown[]) => requestAx(...a),
}))

const openUrl = vi.fn()
vi.mock('@tauri-apps/plugin-opener', () => ({
  openUrl: (...a: unknown[]) => openUrl(...a),
}))

import { PermissionsStep } from '../PermissionsStep'

function resetAll() {
  useAppStore.setState(useAppStore.getInitialState())
  checkMic.mockReset().mockResolvedValue('not_determined')
  requestMic.mockReset().mockResolvedValue(true)
  checkAx.mockReset().mockResolvedValue(false)
  requestAx.mockReset().mockResolvedValue(false)
  openUrl.mockReset().mockResolvedValue(undefined)
}

function mockPlatform(platform: string) {
  Object.defineProperty(window.navigator, 'platform', {
    value: platform,
    configurable: true,
  })
}

describe('PermissionsStep (macOS)', () => {
  beforeEach(() => {
    resetAll()
    mockPlatform('MacIntel')
  })

  it('renders both Microphone and Accessibility cards', () => {
    useAppStore.getState().setMicAuthStatus('not_determined')
    useAppStore.getState().setAccessibilityTrusted(false)
    render(<PermissionsStep />)
    expect(screen.getByText('permissions.microphone.title')).toBeInTheDocument()
    expect(screen.getByText('permissions.accessibility.title')).toBeInTheDocument()
  })

  it('mic Grant button triggers requestMicrophonePermission when not_determined', async () => {
    useAppStore.getState().setMicAuthStatus('not_determined')
    useAppStore.getState().setAccessibilityTrusted(true) // hide AX card buttons
    checkMic.mockResolvedValue('authorized')
    render(<PermissionsStep />)

    const grantButtons = screen.getAllByText('permissions.grant')
    fireEvent.click(grantButtons[0])
    await waitFor(() => expect(requestMic).toHaveBeenCalledOnce())
  })

  it('mic button opens System Settings when status is denied (dialog is one-shot)', async () => {
    useAppStore.getState().setMicAuthStatus('denied')
    useAppStore.getState().setAccessibilityTrusted(true)
    render(<PermissionsStep />)

    // For denied state the button label switches from Grant → Open Settings.
    const openButtons = screen.getAllByText('permissions.openSettings')
    fireEvent.click(openButtons[0])
    await waitFor(() => {
      expect(openUrl).toHaveBeenCalledWith(
        expect.stringMatching(/Privacy_Microphone/),
      )
      expect(requestMic).not.toHaveBeenCalled()
    })
  })

  it('hides the mic Grant button once status is authorized', () => {
    useAppStore.getState().setMicAuthStatus('authorized')
    useAppStore.getState().setAccessibilityTrusted(true)
    render(<PermissionsStep />)

    // Both cards are in granted state — no Grant button anywhere.
    expect(screen.queryAllByText('permissions.grant').length).toBe(0)
    expect(screen.getByText('permissions.microphone.grantedHint')).toBeInTheDocument()
  })

  it('AX Grant button calls requestAccessibilityPermission and re-checks', async () => {
    useAppStore.getState().setMicAuthStatus('authorized') // hide mic Grant
    useAppStore.getState().setAccessibilityTrusted(false)
    checkAx.mockResolvedValueOnce(false).mockResolvedValueOnce(true)
    render(<PermissionsStep />)

    fireEvent.click(screen.getByText('permissions.grant'))
    await waitFor(() => {
      expect(requestAx).toHaveBeenCalledOnce()
      // Hook re-checks after the request to pick up the new value.
      expect(checkAx).toHaveBeenCalled()
    })
  })

  it('AX secondary button opens System Settings → Accessibility', async () => {
    useAppStore.getState().setMicAuthStatus('authorized')
    useAppStore.getState().setAccessibilityTrusted(false)
    render(<PermissionsStep />)

    // The AX card has two buttons: Grant and Open Settings.
    fireEvent.click(screen.getByText('permissions.openSettings'))
    await waitFor(() =>
      expect(openUrl).toHaveBeenCalledWith(
        expect.stringMatching(/Privacy_Accessibility/),
      ),
    )
  })
})

describe('PermissionsStep (non-macOS)', () => {
  beforeEach(() => {
    resetAll()
    mockPlatform('Linux x86_64')
  })

  it('renders nothing actionable so the onboarding flow auto-advances', () => {
    render(<PermissionsStep />)
    expect(screen.queryByText('permissions.microphone.title')).not.toBeInTheDocument()
    expect(screen.queryByText('permissions.accessibility.title')).not.toBeInTheDocument()
  })
})
