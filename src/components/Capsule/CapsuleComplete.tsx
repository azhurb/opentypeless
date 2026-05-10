import { motion } from 'framer-motion'
import { Check } from 'lucide-react'
import { useEffect } from 'react'
import { useAppStore } from '../../stores/appStore'
import { spring } from '../../lib/animations'

const COMPLETE_DURATION_MS = 400

export function CapsuleComplete() {
  const resetRecording = useAppStore((s) => s.resetRecording)
  const setPipelineState = useAppStore((s) => s.setPipelineState)

  useEffect(() => {
    const timer = setTimeout(() => {
      resetRecording()
      setPipelineState('idle')
    }, COMPLETE_DURATION_MS)
    return () => clearTimeout(timer)
  }, [resetRecording, setPipelineState])

  return (
    <motion.div className="relative z-10 flex items-center justify-center h-9 px-3">
      <motion.div
        initial={{ scale: 0, opacity: 0 }}
        animate={{ scale: 1, opacity: 1 }}
        transition={spring.smooth}
      >
        <Check size={14} className="text-white" />
      </motion.div>
    </motion.div>
  )
}
