import { useState, useEffect } from 'react'
import { motion, AnimatePresence } from 'framer-motion'
import { MicOff } from 'lucide-react'
import { openUrl } from '@tauri-apps/plugin-opener'
import { useTranslation } from 'react-i18next'
import { useAppStore } from '../../stores/appStore'

const MIC_PREFS_URL = 'x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone'

export function MicDeniedBanner() {
  const { t } = useTranslation()
  const micAuthStatus = useAppStore((s) => s.micAuthStatus)
  const isMac =
    typeof navigator !== 'undefined' && navigator.platform.toUpperCase().indexOf('MAC') >= 0
  const [dismissed, setDismissed] = useState(false)

  const show = isMac && (micAuthStatus === 'denied' || micAuthStatus === 'restricted') && !dismissed

  // Re-show banner if the status flips back to denied (e.g. user tried to
  // dictate and the hard-gate fired again).
  useEffect(() => {
    if (micAuthStatus === 'denied' || micAuthStatus === 'restricted') setDismissed(false)
  }, [micAuthStatus])

  return (
    <AnimatePresence>
      {show && (
        <motion.div
          initial={{ height: 0, opacity: 0 }}
          animate={{ height: 'auto', opacity: 1 }}
          exit={{ height: 0, opacity: 0 }}
          transition={{ duration: 0.2 }}
          className="overflow-hidden"
        >
          <div className="flex items-center gap-2 px-4 py-2 bg-red-500/10 border-b border-red-500/20">
            <MicOff size={14} className="text-red-500 shrink-0" />
            <span className="text-[12px] text-text-primary flex-1">
              {t('permissions.microphone.denied')}
            </span>
            <button
              onClick={() => openUrl(MIC_PREFS_URL)}
              className="px-3 py-1 text-[11px] font-medium text-white bg-accent rounded-full border-none cursor-pointer hover:bg-accent-hover transition-colors shrink-0"
            >
              {t('permissions.openSettings')}
            </button>
            <button
              onClick={() => setDismissed(true)}
              className="text-text-tertiary text-[12px] border-none bg-transparent cursor-pointer hover:text-text-secondary shrink-0"
              aria-label="Dismiss"
            >
              ✕
            </button>
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  )
}
