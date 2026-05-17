import { useEffect } from 'react'
import { motion } from 'framer-motion'
import { useTranslation } from 'react-i18next'
import { useAppStore } from '../../stores/appStore'

const PERMISSION_ERROR_KEYS: Record<string, string> = {
  ACCESSIBILITY_REQUIRED: 'capsule.accessibilityRequired',
  MICROPHONE_DENIED: 'capsule.microphoneDenied',
}

export function CapsuleError() {
  const { t } = useTranslation()
  const pipelineError = useAppStore((s) => s.pipelineError)
  const setPipelineError = useAppStore((s) => s.setPipelineError)
  const resetRecording = useAppStore((s) => s.resetRecording)

  // Permission errors are actionable — keep them on screen so the user can
  // tap the capsule to open System Settings. Transient errors still auto-clear.
  const isPermissionError = pipelineError !== null && pipelineError in PERMISSION_ERROR_KEYS

  useEffect(() => {
    if (isPermissionError) return
    const timer = setTimeout(() => {
      setPipelineError(null)
      const currentState = useAppStore.getState().pipelineState
      if (currentState === 'idle') {
        resetRecording()
      }
    }, 2500)
    return () => clearTimeout(timer)
  }, [setPipelineError, resetRecording, pipelineError, isPermissionError])

  const message =
    pipelineError && pipelineError in PERMISSION_ERROR_KEYS
      ? t(PERMISSION_ERROR_KEYS[pipelineError])
      : pipelineError || 'An error occurred'

  return (
    <motion.div
      className="relative z-10 flex items-center gap-2 h-9 px-3 max-w-[200px]"
      initial={{ opacity: 0, x: -4 }}
      animate={{ opacity: 1, x: 0 }}
      transition={{ duration: 0.3, ease: 'easeOut' }}
    >
      {/* White dot */}
      <motion.div className="w-2 h-2 rounded-full bg-white/80 flex-shrink-0" />
      <p className="text-[11px] text-white truncate flex-1">{message}</p>
    </motion.div>
  )
}
