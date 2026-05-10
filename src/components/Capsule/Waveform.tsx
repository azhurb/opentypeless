import { useEffect, useRef } from 'react'
import { useReducedMotion } from 'framer-motion'
import { useAppStore } from '../../stores/appStore'

const BAR_COUNT = 7
const MIN_HEIGHT = 3
const MAX_HEIGHT = 16

// RMS of normalized f32 samples for conversational voice typically lands in
// 0.02–0.10. Gain + sqrt curve maps that range across most of the visible bar
// height; NOISE_FLOOR keeps idle hiss from waking the bars.
const VOLUME_GAIN = 6
const NOISE_FLOOR = 0.01

// Asymmetric smoothing: rise quickly toward incoming peaks, fall slowly so
// brief silences between words don't collapse the bars.
const ATTACK = 0.5
const RELEASE = 0.18

// Center bars react more than edge bars, giving the column a voice-shaped
// envelope rather than seven bars moving in lockstep.
const BAR_RESPONSIVITY = [0.55, 0.75, 0.9, 1.0, 0.9, 0.75, 0.55]

export function Waveform() {
  const barsRef = useRef<(HTMLDivElement | null)[]>([])
  const rafRef = useRef<number>(0)
  const reduced = useReducedMotion()

  useEffect(() => {
    if (reduced) {
      barsRef.current.forEach((bar) => {
        if (!bar) return
        bar.style.height = `${(MIN_HEIGHT + MAX_HEIGHT) / 2}px`
        bar.style.opacity = '0.7'
      })
      return
    }

    let smoothed = 0

    const animate = () => {
      const raw = useAppStore.getState().audioVolume
      const above = Math.max(0, raw - NOISE_FLOOR)
      const target = Math.min(1, Math.sqrt(above) * VOLUME_GAIN)
      const k = target > smoothed ? ATTACK : RELEASE
      smoothed += (target - smoothed) * k

      barsRef.current.forEach((bar, i) => {
        if (!bar) return
        const ambient = 0.04 + Math.sin(Date.now() / 600 + i * 0.7) * 0.03
        const level = Math.max(ambient, smoothed * BAR_RESPONSIVITY[i])
        bar.style.height = `${MIN_HEIGHT + (MAX_HEIGHT - MIN_HEIGHT) * level}px`
        bar.style.opacity = `${0.45 + 0.55 * level}`
      })
      rafRef.current = requestAnimationFrame(animate)
    }

    rafRef.current = requestAnimationFrame(animate)
    return () => cancelAnimationFrame(rafRef.current)
  }, [reduced])

  return (
    <div className="flex items-center justify-center gap-[3px] h-4">
      {Array.from({ length: BAR_COUNT }).map((_, i) => (
        <div
          key={i}
          ref={(el) => {
            barsRef.current[i] = el
          }}
          className="w-[2px] rounded-full bg-white/80"
          style={{
            height: `${MIN_HEIGHT}px`,
            opacity: 0.5,
          }}
        />
      ))}
    </div>
  )
}
