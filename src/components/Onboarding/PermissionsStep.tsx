import { useCallback, useEffect, useState } from 'react'
import { Mic, ShieldCheck, ShieldAlert } from 'lucide-react'
import { openUrl } from '@tauri-apps/plugin-opener'
import { useTranslation } from 'react-i18next'
import { useAppStore } from '../../stores/appStore'
import {
  checkAccessibilityPermission,
  checkMicrophonePermission,
  requestAccessibilityPermission,
  requestMicrophonePermission,
} from '../../lib/tauri'

const MIC_PREFS_URL = 'x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone'
const AX_PREFS_URL = 'x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility'

export function PermissionsStep() {
  const { t } = useTranslation()
  const isMac =
    typeof navigator !== 'undefined' && navigator.platform.toUpperCase().indexOf('MAC') >= 0
  const micAuthStatus = useAppStore((s) => s.micAuthStatus)
  const setMicAuthStatus = useAppStore((s) => s.setMicAuthStatus)
  const accessibilityTrusted = useAppStore((s) => s.accessibilityTrusted)
  const setAccessibilityTrusted = useAppStore((s) => s.setAccessibilityTrusted)
  const [axClicked, setAxClicked] = useState(false)

  // Sync from native state on mount so any grants done while the app was
  // backgrounded between steps are picked up.
  useEffect(() => {
    if (!isMac) return
    checkMicrophonePermission().then(setMicAuthStatus)
    checkAccessibilityPermission().then(setAccessibilityTrusted)
  }, [isMac, setMicAuthStatus, setAccessibilityTrusted])

  const handleGrantMic = useCallback(async () => {
    if (micAuthStatus === 'denied' || micAuthStatus === 'restricted') {
      // Dialog is one-shot per install — once denied, send the user to Settings.
      await openUrl(MIC_PREFS_URL)
      return
    }
    await requestMicrophonePermission()
    const status = await checkMicrophonePermission()
    setMicAuthStatus(status)
  }, [micAuthStatus, setMicAuthStatus])

  const handleGrantAx = useCallback(async () => {
    setAxClicked(true)
    await requestAccessibilityPermission()
    const trusted = await checkAccessibilityPermission()
    setAccessibilityTrusted(trusted)
  }, [setAccessibilityTrusted])

  if (!isMac) {
    return (
      <div className="text-center py-8 text-[13px] text-text-tertiary">
        {/* No-op step on non-macOS — Linux/Windows don't need these grants. */}
      </div>
    )
  }

  const micGranted = micAuthStatus === 'authorized'
  const micBlocked = micAuthStatus === 'denied' || micAuthStatus === 'restricted'

  return (
    <div className="flex flex-col gap-3 py-2">
      <p className="text-[13px] text-text-secondary text-center pb-1">
        {t('permissions.description')}
      </p>

      {/* Microphone */}
      <div
        className={`w-full px-3 py-2.5 rounded-[10px] border ${
          micGranted
            ? 'bg-green-500/10 border-green-500/20'
            : micBlocked
              ? 'bg-red-500/10 border-red-500/20'
              : 'bg-amber-500/10 border-amber-500/20'
        }`}
      >
        <div className="flex items-center gap-2 mb-2">
          {micGranted ? (
            <ShieldCheck size={14} className="text-green-500 shrink-0" />
          ) : (
            <Mic size={14} className={micBlocked ? 'text-red-500 shrink-0' : 'text-amber-500 shrink-0'} />
          )}
          <span className="text-[12px] font-medium text-text-primary">
            {t('permissions.microphone.title')}
          </span>
          <span className="text-[11px] text-text-tertiary ml-auto">
            {micGranted
              ? t('permissions.microphone.grantedHint')
              : t('permissions.microphone.required')}
          </span>
        </div>
        {!micGranted && (
          <button
            onClick={handleGrantMic}
            className="w-full py-1.5 text-[12px] font-medium text-white bg-accent rounded-[8px] border-none cursor-pointer hover:bg-accent-hover transition-colors"
          >
            {micBlocked ? t('permissions.openSettings') : t('permissions.grant')}
          </button>
        )}
      </div>

      {/* Accessibility */}
      <div
        className={`w-full px-3 py-2.5 rounded-[10px] border ${
          accessibilityTrusted
            ? 'bg-green-500/10 border-green-500/20'
            : 'bg-amber-500/10 border-amber-500/20'
        }`}
      >
        <div className="flex items-center gap-2 mb-2">
          {accessibilityTrusted ? (
            <ShieldCheck size={14} className="text-green-500 shrink-0" />
          ) : (
            <ShieldAlert size={14} className="text-amber-500 shrink-0" />
          )}
          <span className="text-[12px] font-medium text-text-primary">
            {t('permissions.accessibility.title')}
          </span>
          <span className="text-[11px] text-text-tertiary ml-auto">
            {accessibilityTrusted
              ? t('permissions.accessibility.grantedHint')
              : t('permissions.accessibility.required')}
          </span>
        </div>
        {!accessibilityTrusted && (
          <>
            <div className="flex gap-2">
              <button
                onClick={handleGrantAx}
                className="flex-1 py-1.5 text-[12px] font-medium text-white bg-accent rounded-[8px] border-none cursor-pointer hover:bg-accent-hover transition-colors"
              >
                {t('permissions.grant')}
              </button>
              <button
                onClick={() => openUrl(AX_PREFS_URL)}
                className="px-3 py-1.5 text-[12px] font-medium text-text-secondary bg-bg-tertiary rounded-[8px] border-none cursor-pointer hover:text-text-primary transition-colors"
              >
                {t('permissions.openSettings')}
              </button>
            </div>
            {axClicked && (
              <p className="text-[10px] text-text-tertiary mt-1.5 text-center">
                {t('permissions.accessibility.afterClickHint')}
              </p>
            )}
          </>
        )}
      </div>
    </div>
  )
}
