import { useEffect } from 'react'
import { motion } from 'framer-motion'
import { useTranslation } from 'react-i18next'
import { useAppStore } from '../../stores/appStore'

// A bit longer than the 2.5s error pill so the user has time to read it and
// place their cursor before pasting. Clicking the pill dismisses it early
// (handled by the parent Capsule's pointer handler); a new dictation also
// clears it.
const TIP_DURATION_MS = 4000

const isMac =
  typeof navigator !== 'undefined' && navigator.platform.toUpperCase().indexOf('MAC') >= 0
// Detection is macOS-only today, so this is effectively always ⌘V — computed
// from platform so it stays correct if detection is extended to other OSes.
const PASTE_SHORTCUT = isMac ? '⌘V' : 'Ctrl+V'

export function CapsuleClipboardTip() {
  const { t } = useTranslation()
  const setClipboardTip = useAppStore((s) => s.setClipboardTip)

  useEffect(() => {
    const timer = setTimeout(() => setClipboardTip(false), TIP_DURATION_MS)
    return () => clearTimeout(timer)
  }, [setClipboardTip])

  return (
    <motion.div
      className="relative z-10 flex items-center gap-2 h-9 px-3 max-w-[220px]"
      initial={{ opacity: 0, x: -4 }}
      animate={{ opacity: 1, x: 0 }}
      transition={{ duration: 0.3, ease: 'easeOut' }}
    >
      {/* White dot — matches the error pill's layout */}
      <motion.div className="w-2 h-2 rounded-full bg-white/80 flex-shrink-0" />
      <p className="text-[11px] text-white truncate flex-1">
        {t('capsule.clipboardTip', { shortcut: PASTE_SHORTCUT })}
      </p>
    </motion.div>
  )
}
