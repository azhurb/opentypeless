import { useEffect, useRef } from 'react'
import { AnimatePresence, motion } from 'framer-motion'
import { useTranslation } from 'react-i18next'
import { spring } from '../lib/animations'

interface Props {
  open: boolean
  message: string
  /** Defaults to `common.confirm`. */
  confirmLabel?: string
  /** Defaults to `common.cancel`. */
  cancelLabel?: string
  /** Styles the confirm button as destructive. Irreversible deletes should set it. */
  destructive?: boolean
  onConfirm: () => void
  onCancel: () => void
}

/**
 * In-app replacement for `window.confirm`, which cannot be used here.
 *
 * WKWebView only shows JS dialogs when the host implements `WKUIDelegate`'s
 * `runJavaScriptConfirmPanelWithMessage:` — and wry implements no such method, so
 * on macOS `window.confirm()` returns falsy immediately without displaying
 * anything. Any guard written as `if (!window.confirm(...)) return` therefore
 * always takes the early return, which is how "Clear All History" came to
 * silently do nothing. Use this component instead; never `window.confirm`.
 */
export function ConfirmDialog({
  open,
  message,
  confirmLabel,
  cancelLabel,
  destructive = false,
  onConfirm,
  onCancel,
}: Props) {
  const { t } = useTranslation()
  // Destructive actions focus Cancel, so a stray Enter/Space dismisses rather
  // than deletes.
  const cancelRef = useRef<HTMLButtonElement>(null)

  useEffect(() => {
    if (!open) return
    cancelRef.current?.focus()
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        onCancel()
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [open, onCancel])

  return (
    <AnimatePresence>
      {open && (
        <motion.div
          className="fixed inset-0 z-[10000] flex items-center justify-center bg-black/40 px-6"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          onClick={onCancel}
        >
          <motion.div
            data-testid="confirm-dialog"
            role="dialog"
            aria-modal="true"
            initial={{ opacity: 0, scale: 0.95, y: 8 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.95, y: 8 }}
            transition={spring.jellyGentle}
            // Clicks inside must not reach the backdrop's cancel handler.
            onClick={(e) => e.stopPropagation()}
            className="w-full max-w-[320px] p-4 bg-bg-secondary border border-border rounded-[14px] shadow-xl"
          >
            <p className="text-[13px] text-text-primary leading-relaxed whitespace-pre-line">
              {message}
            </p>
            <div className="flex items-center justify-end gap-2 mt-4">
              <button
                ref={cancelRef}
                data-testid="confirm-dialog-cancel"
                onClick={onCancel}
                className="px-3 py-1.5 text-[12px] text-text-secondary hover:text-text-primary bg-transparent border-none cursor-pointer rounded-[10px] hover:bg-bg-tertiary transition-colors"
              >
                {cancelLabel ?? t('common.cancel')}
              </button>
              <button
                data-testid="confirm-dialog-confirm"
                onClick={onConfirm}
                className={`px-3 py-1.5 text-[12px] text-white rounded-[10px] border-none cursor-pointer hover:opacity-90 transition-opacity ${
                  destructive ? 'bg-error' : 'bg-accent'
                }`}
              >
                {confirmLabel ?? t('common.confirm')}
              </button>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  )
}
