import { useEffect } from 'react'
import { motion } from 'framer-motion'
import { useTranslation } from 'react-i18next'
import { useAppStore } from '../../stores/appStore'

// 3s sits between the 2.5s error pill and the 4s clipboard tip: long enough to
// read six words and register the shortcut, short enough not to linger over work
// the user has already moved on from. Modelled on CapsuleClipboardTip rather than
// CapsuleComplete — Complete runs 400ms and renders only an icon, which is far too
// short to read a label. Clicking the pill dismisses it early (handled by the
// parent Capsule's pointer handler); a new dictation also clears it.
const TIP_DURATION_MS = 3000

const isMac =
  typeof navigator !== 'undefined' && navigator.platform.toUpperCase().indexOf('MAC') >= 0
const UNDO_SHORTCUT = isMac ? '⌘Z' : 'Ctrl+Z'

export function CapsuleEditedTip() {
  const { t } = useTranslation()
  const setEditedTip = useAppStore((s) => s.setEditedTip)

  useEffect(() => {
    const timer = setTimeout(() => setEditedTip(false), TIP_DURATION_MS)
    return () => clearTimeout(timer)
  }, [setEditedTip])

  return (
    <motion.div
      className="relative z-10 flex items-center gap-2 h-9 px-3 max-w-[220px]"
      initial={{ opacity: 0, x: -4 }}
      animate={{ opacity: 1, x: 0 }}
      transition={{ duration: 0.3, ease: 'easeOut' }}
    >
      {/* Amber dot — carries the same "this changed text you had" signal as the
          mode ring that was up a moment ago, where the clipboard tip's white dot
          would read as ordinary progress. */}
      <motion.div className="w-2 h-2 rounded-full bg-warning flex-shrink-0" />
      <p className="text-[11px] text-white truncate flex-1">
        {t('capsule.editedTip', { shortcut: UNDO_SHORTCUT })}
      </p>
    </motion.div>
  )
}
