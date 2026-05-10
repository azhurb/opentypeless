import { useEffect, useRef } from 'react'
import { useAppStore, type PipelineState } from '../stores/appStore'

interface CapsuleSize {
  width: number
  height: number
}

function getSizeForState(
  state: PipelineState,
  expanded: boolean,
  hasError: boolean,
  contextMenuOpen: boolean,
): CapsuleSize {
  if (contextMenuOpen) return { width: 220, height: 220 }
  if (hasError) return { width: 200, height: 36 }
  if (expanded) return { width: 220, height: 90 }
  switch (state) {
    case 'idle':
      return { width: 36, height: 36 }
    case 'recording':
      return { width: 200, height: 36 }
    case 'transcribing':
    case 'polishing':
      return { width: 220, height: 36 }
    case 'outputting':
      // Match the polishing width so the window doesn't shrink mid-exit
      // and clip the polishing capsule's right edge during AnimatePresence
      // transition. Outputting's content (single centered checkmark) is
      // ~38px, so it sits comfortably inside the wider window.
      return { width: 220, height: 36 }
    default:
      return { width: 36, height: 36 }
  }
}

export function useCapsuleResize() {
  const pipelineState = useAppStore((s) => s.pipelineState)
  const capsuleExpanded = useAppStore((s) => s.capsuleExpanded)
  const pipelineError = useAppStore((s) => s.pipelineError)
  const contextMenuOpen = useAppStore((s) => s.contextMenuOpen)
  const setContextMenuReady = useAppStore((s) => s.setContextMenuReady)
  const capsuleAutoHide = useAppStore((s) => s.config.capsule_auto_hide)
  const configLoaded = useAppStore((s) => s.configLoaded)
  const initialized = useRef(false)
  const prevWindowSize = useRef<{ width: number; height: number } | null>(null)
  const prevState = useRef<PipelineState>('idle')
  const prevAutoHide = useRef(false)

  const hasError = pipelineError !== null

  useEffect(() => {
    const size = getSizeForState(pipelineState, capsuleExpanded, hasError, contextMenuOpen)
    const windowWidth = size.width + 24
    const windowHeight = size.height + 24

    import('@tauri-apps/api/window')
      .then(
        async ({
          getCurrentWindow,
          LogicalSize,
          LogicalPosition,
          currentMonitor,
          monitorFromPoint,
          primaryMonitor,
          cursorPosition,
        }) => {
          const win = getCurrentWindow()

          // Pick the monitor under the cursor so the capsule appears on the
          // user's active screen on multi-monitor setups. Falls back to the
          // window's current monitor.
          async function placeBottomCenterOfActiveMonitor() {
            let monitor = null
            try {
              const cursor = await cursorPosition()
              // tao's cursorPosition() returns physical coords scaled by the
              // primary monitor, but monitorFromPoint() checks against
              // CGDisplayBounds (logical). On Retina they differ by the scale
              // factor, so we must convert before lookup.
              const primary = await primaryMonitor().catch(() => null)
              const scale = primary?.scaleFactor ?? 1
              monitor = await monitorFromPoint(cursor.x / scale, cursor.y / scale)
            } catch {
              /* ignore */
            }
            if (!monitor) {
              monitor = await currentMonitor().catch(() => null)
            }
            if (!monitor) return
            const sw = monitor.size.width / monitor.scaleFactor
            const sh = monitor.size.height / monitor.scaleFactor
            const mx = monitor.position.x / monitor.scaleFactor
            const my = monitor.position.y / monitor.scaleFactor
            const x = Math.round(mx + sw / 2 - windowWidth / 2)
            const y = Math.round(my + sh - windowHeight - 80)
            await win.setPosition(new LogicalPosition(x, y)).catch(() => {})
          }

          // Auto-hide: show window when leaving idle, hide when entering idle
          const becameIdle = prevState.current !== 'idle' && pipelineState === 'idle'
          const leftIdle = prevState.current === 'idle' && pipelineState !== 'idle'
          prevState.current = pipelineState

          if (capsuleAutoHide && !contextMenuOpen && !capsuleExpanded) {
            if (leftIdle) {
              // Reposition first (while still hidden) so it appears on the
              // monitor where the user is now, not where it last sat.
              await placeBottomCenterOfActiveMonitor()
              await win.show().catch(() => {})
            } else if (becameIdle && initialized.current) {
              // Hide window when returning to idle (after initial mount)
              await win.hide().catch(() => {})
              prevAutoHide.current = capsuleAutoHide
              return
            }
          }

          // If auto-hide was just disabled and we're idle, show the capsule
          if (prevAutoHide.current && !capsuleAutoHide && pipelineState === 'idle') {
            await win.show().catch(() => {})
          }
          prevAutoHide.current = capsuleAutoHide

          if (!initialized.current) {
            // Wait for the persisted config to load before deciding whether to
            // show the capsule. Otherwise the default `capsule_auto_hide: false`
            // wins the race against `getConfig()` and the capsule briefly
            // appears even when the user has opted into auto-hide.
            if (!configLoaded) return

            // First mount: size, position on the cursor's monitor, then show
            await win.setSize(new LogicalSize(windowWidth, windowHeight)).catch(() => {})
            await placeBottomCenterOfActiveMonitor()
            if (capsuleAutoHide) {
              // Belt-and-braces: tauri.conf.json marks the capsule
              // `visible: false`, but on macOS the setSize/setPosition calls
              // above can briefly surface the window before any show() call.
              // Re-asserting hidden here is a no-op when the OS already kept
              // it hidden, and pulls it back when it didn't.
              await win.hide().catch(() => {})
            } else {
              await win.show().catch(() => {})
            }
            initialized.current = true
            prevWindowSize.current = { width: windowWidth, height: windowHeight }
            return
          }

          // Subsequent resizes: left edge + vertical center stay fixed.
          // Since content is always padded 12px each side, the capsule at x=12
          // is identical to a centered capsule — so the mic icon never moves.
          const prev = prevWindowSize.current
          if (prev) {
            const pos = await win.outerPosition().catch(() => null)
            if (pos) {
              const monitor = await currentMonitor()
              const scale = monitor?.scaleFactor ?? 1
              const oldLeftX = pos.x / scale
              const oldCenterY = pos.y / scale + prev.height / 2
              const newX = Math.round(oldLeftX)
              const newY = Math.round(oldCenterY - windowHeight / 2)
              await win.setPosition(new LogicalPosition(newX, newY)).catch(() => {})
              await win.setSize(new LogicalSize(windowWidth, windowHeight)).catch(() => {})
            } else {
              await win.setSize(new LogicalSize(windowWidth, windowHeight)).catch(() => {})
            }
          } else {
            await win.setSize(new LogicalSize(windowWidth, windowHeight)).catch(() => {})
          }

          prevWindowSize.current = { width: windowWidth, height: windowHeight }

          // Signal that the window has finished resizing for context menu
          if (contextMenuOpen) {
            setContextMenuReady(true)
          }
        },
      )
      .catch(() => {})
  }, [
    pipelineState,
    capsuleExpanded,
    hasError,
    contextMenuOpen,
    capsuleAutoHide,
    configLoaded,
    setContextMenuReady,
  ])

  return getSizeForState(pipelineState, capsuleExpanded, hasError, contextMenuOpen)
}
