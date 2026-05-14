import { useEffect, useRef, useState } from 'react'
import { AnimatePresence, motion } from 'framer-motion'
import { useTranslation } from 'react-i18next'
import { useAppStore } from '../../stores/appStore'
import { correctionUndo } from '../../lib/tauri'
import { spring } from '../../lib/animations'

type Mode = 'idle' | 'undone'

export function CorrectionToast() {
  const suggestion = useAppStore((s) => s.correctionSuggestion)
  const clearSuggestion = useAppStore((s) => s.setCorrectionSuggestion)
  const pipelineState = useAppStore((s) => s.pipelineState)
  const pipelineError = useAppStore((s) => s.pipelineError)
  const { t } = useTranslation()
  const [progress, setProgress] = useState(0)
  const [mode, setMode] = useState<Mode>('idle')
  const startedAtRef = useRef<number | null>(null)
  const rafRef = useRef<number | null>(null)
  const undoTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const undoInFlightRef = useRef(false)

  const hasError = pipelineError !== null
  // Errors take precedence over the correction toast. Drop the suggestion
  // when an error appears so the two pills don't overlap.
  useEffect(() => {
    if (suggestion && (pipelineState !== 'idle' || hasError)) {
      clearSuggestion(null)
      setMode('idle')
      setProgress(0)
    }
  }, [pipelineState, suggestion, hasError, clearSuggestion])

  useEffect(() => {
    if (!suggestion || mode !== 'idle') return
    startedAtRef.current = performance.now()
    const tick = () => {
      const start = startedAtRef.current
      if (start === null || undoInFlightRef.current) return
      const elapsed = performance.now() - start
      const p = Math.min(1, elapsed / suggestion.autoConfirmMs)
      setProgress(p)
      if (p < 1) {
        rafRef.current = requestAnimationFrame(tick)
      } else {
        clearSuggestion(null)
        setProgress(0)
      }
    }
    rafRef.current = requestAnimationFrame(tick)
    return () => {
      if (rafRef.current !== null) cancelAnimationFrame(rafRef.current)
      rafRef.current = null
    }
  }, [suggestion, mode, clearSuggestion])

  useEffect(() => {
    return () => {
      if (undoTimerRef.current !== null) {
        clearTimeout(undoTimerRef.current)
        undoTimerRef.current = null
      }
    }
  }, [])

  const handleUndo = async () => {
    if (!suggestion) return
    undoInFlightRef.current = true
    if (rafRef.current !== null) {
      cancelAnimationFrame(rafRef.current)
      rafRef.current = null
    }
    try {
      await correctionUndo(suggestion.rowId)
      setMode('undone')
      if (undoTimerRef.current !== null) clearTimeout(undoTimerRef.current)
      undoTimerRef.current = setTimeout(() => {
        undoTimerRef.current = null
        undoInFlightRef.current = false
        clearSuggestion(null)
        setMode('idle')
        setProgress(0)
      }, 1000)
    } catch (e) {
      console.error('correctionUndo failed:', e)
      undoInFlightRef.current = false
      clearSuggestion(null)
      setMode('idle')
    }
  }

  return (
    <AnimatePresence>
      {suggestion && !hasError && (
        <motion.div
          initial={{ opacity: 0, y: 8, scale: 0.96 }}
          animate={{ opacity: 1, y: 0, scale: 1 }}
          exit={{ opacity: 0, y: 8, scale: 0.96 }}
          transition={spring.jellyGentle}
          className="pointer-events-auto absolute left-3 right-3 top-1/2 -translate-y-1/2 h-9 flex items-center gap-3 px-4 bg-black/90 backdrop-blur-sm rounded-full text-white text-[13px] shadow-2xl overflow-hidden whitespace-nowrap"
          role="status"
          aria-live="polite"
        >
          <span className="select-none flex-1 min-w-0 truncate">
            {mode === 'undone'
              ? t('correction.removed', { new: suggestion.new })
              : t('correction.replaced', { old: suggestion.old, new: suggestion.new })}
          </span>
          {mode !== 'undone' && (
            <button
              type="button"
              onClick={handleUndo}
              className="px-3 py-1 bg-white/15 hover:bg-white/25 active:bg-white/10 transition-colors rounded-full text-white text-[12px] font-medium border-none cursor-pointer"
            >
              {t('correction.undo')}
            </button>
          )}
          <div
            className="absolute left-0 bottom-0 h-[2px] bg-white/60"
            style={{ width: `${progress * 100}%` }}
            aria-hidden="true"
          />
        </motion.div>
      )}
    </AnimatePresence>
  )
}
