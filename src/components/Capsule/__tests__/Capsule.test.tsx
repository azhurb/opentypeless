/**
 * Capsule shell behavior for selected-text editing:
 *   - the amber mode ring is on the pill for the whole run when this dictation
 *     will replace a selection, and absent otherwise
 *   - the ring never survives into idle, because the pill in idle is the plain
 *     mic and an amber ring there would be a permanent false alarm
 *   - the edited tip renders, and ranks below the clipboard tip
 *
 * The ring is a CSS class rather than a component, so the class name is the only
 * observable. It has to be a class on the *same* element as jelly-capsule-active:
 * the ring is an inset box-shadow that replaces that class's shadow stack, and a
 * separate wrapper element would either be clipped by `overflow: hidden` or
 * change the pill's size.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import React from 'react'
import { useAppStore, type PipelineState } from '../../../stores/appStore'

afterEach(() => {
  cleanup()
})

const MOTION_PROPS = new Set(['initial', 'animate', 'exit', 'transition', 'whileHover', 'whileTap'])
vi.mock('framer-motion', () => ({
  AnimatePresence: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  useReducedMotion: () => false,
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

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: 'en', changeLanguage: vi.fn() },
  }),
}))

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn().mockResolvedValue(undefined) }))

// The resize hook drives the real capsule NSWindow; it has its own test and
// nothing here depends on the size it returns.
vi.mock('../../../hooks/useCapsuleResize', () => ({
  useCapsuleResize: () => ({ width: 220, height: 36 }),
}))

import { Capsule } from '../index'

/** The pill itself — the element carrying the jelly-capsule-* classes. */
function pill(container: HTMLElement): HTMLElement {
  const el = container.querySelector('.rounded-full.pointer-events-auto')
  if (!el) throw new Error('capsule pill not found')
  return el as HTMLElement
}

function setUp(state: PipelineState, editingSelection: boolean) {
  useAppStore.setState({ ...useAppStore.getInitialState(), pipelineState: state, editingSelection })
}

describe('Capsule — selected-text mode ring', () => {
  beforeEach(() => {
    useAppStore.setState(useAppStore.getInitialState())
  })

  it.each<PipelineState>(['recording', 'transcribing', 'polishing', 'outputting'])(
    'shows the ring during %s when a selection is being edited',
    (state) => {
      setUp(state, true)
      const { container } = render(<Capsule />)
      expect(pill(container).className).toContain('jelly-capsule-editing')
      // Must compose with the active pill, not replace it.
      expect(pill(container).className).toContain('jelly-capsule-active')
    },
  )

  it.each<PipelineState>(['recording', 'transcribing', 'polishing', 'outputting'])(
    'omits the ring during %s for an ordinary dictation',
    (state) => {
      setUp(state, false)
      const { container } = render(<Capsule />)
      expect(pill(container).className).not.toContain('jelly-capsule-editing')
    },
  )

  it('never rings the idle pill, even if the flag is somehow still set', () => {
    // Rust clears the flag at the start of every run and useTauriEvents clears it
    // on idle, but the ring must not depend on either having fired: an amber ring
    // on the resting mic would read as a permanent warning.
    setUp('idle', true)
    const { container } = render(<Capsule />)
    expect(pill(container).className).not.toContain('jelly-capsule-editing')
  })

  it('does not ring the error pill', () => {
    useAppStore.setState({
      ...useAppStore.getInitialState(),
      pipelineState: 'idle',
      editingSelection: true,
      pipelineError: 'Polish failed',
    })
    const { container } = render(<Capsule />)
    expect(pill(container).className).toContain('jelly-capsule-error')
    expect(pill(container).className).not.toContain('jelly-capsule-editing')
  })
})

describe('Capsule — edited tip', () => {
  beforeEach(() => {
    useAppStore.setState(useAppStore.getInitialState())
  })

  it('renders the edited tip when the flag is set', () => {
    useAppStore.setState({ ...useAppStore.getInitialState(), editedTip: true })
    const { container } = render(<Capsule />)
    expect(container.textContent).toContain('capsule.editedTip')
  })

  it('yields to the clipboard tip', () => {
    // A paste that never landed needs the user to act; a successful edit only
    // needs acknowledging. Rust also guards the pairing, so this is belt and
    // braces for the case where both flags are somehow set.
    useAppStore.setState({
      ...useAppStore.getInitialState(),
      editedTip: true,
      clipboardTip: true,
    })
    const { container } = render(<Capsule />)
    expect(container.textContent).toContain('capsule.clipboardTip')
    expect(container.textContent).not.toContain('capsule.editedTip')
  })

  it('yields to a permission error', () => {
    useAppStore.setState({
      ...useAppStore.getInitialState(),
      editedTip: true,
      pipelineError: 'ACCESSIBILITY_REQUIRED',
    })
    const { container } = render(<Capsule />)
    expect(container.textContent).not.toContain('capsule.editedTip')
  })
})
