import { useState, useCallback, useEffect, useMemo, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { useAppStore } from '../../stores/appStore'
import type { HotkeyMode } from '../../stores/appStore'
import { updateHotkey, pauseHotkey, resumeHotkey } from '../../lib/tauri'
import { SegmentedControl } from './shared/SegmentedControl'
import { Toggle } from './shared/Toggle'
import { ConfirmDialog } from '../ConfirmDialog'

// Keys that can be used as hotkeys without a modifier
const STANDALONE_KEYS = new Set([
  'Space',
  'Tab',
  'Enter',
  'Backspace',
  'Escape',
  'Delete',
  'Insert',
  'Home',
  'End',
  'PageUp',
  'PageDown',
  'Up',
  'Down',
  'Left',
  'Right',
  'F1',
  'F2',
  'F3',
  'F4',
  'F5',
  'F6',
  'F7',
  'F8',
  'F9',
  'F10',
  'F11',
  'F12',
])

// Age limits offered for stored history. 0 = keep forever (the default, so an
// upgrade never silently deletes anything).
const RETENTION_OPTIONS = [0, 7, 30, 90]

function HotkeyRecorder() {
  const config = useAppStore((s) => s.config)
  const updateConfig = useAppStore((s) => s.updateConfig)
  const { t } = useTranslation()
  const [recording, setRecording] = useState(false)
  const [pending, setPending] = useState<string | null>(null)
  const [modifierHint, setModifierHint] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const autoConfirmTimer = useRef<ReturnType<typeof setTimeout> | null>(null)

  const confirmHotkey = useCallback(
    (hotkey: string) => {
      setRecording(false)
      setError(null)
      setModifierHint(null)
      updateHotkey(hotkey)
        .then(() => {
          updateConfig({ hotkey })
          setPending(null)
        })
        .catch((e) => {
          setError(String(e))
          setPending(null)
          resumeHotkey().catch(() => {})
        })
    },
    [updateConfig],
  )

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      e.preventDefault()
      e.stopPropagation()

      // Build modifier prefix
      const parts: string[] = []
      if (e.ctrlKey) parts.push('Ctrl')
      if (e.altKey) parts.push('Alt')
      if (e.shiftKey) parts.push('Shift')
      if (e.metaKey) parts.push('Meta')

      // If only modifier keys are pressed, show hint like "Alt+..."
      if (['Control', 'Shift', 'Alt', 'Meta'].includes(e.key)) {
        setModifierHint(parts.length > 0 ? parts.join('+') + '+...' : null)
        return
      }

      setModifierHint(null)

      const keyMap: Record<string, string> = {
        ' ': 'Space',
        Tab: 'Tab',
        Enter: 'Enter',
        Backspace: 'Backspace',
        Escape: 'Escape',
        Delete: 'Delete',
        Insert: 'Insert',
        Home: 'Home',
        End: 'End',
        PageUp: 'PageUp',
        PageDown: 'PageDown',
        ArrowUp: 'Up',
        ArrowDown: 'Down',
        ArrowLeft: 'Left',
        ArrowRight: 'Right',
      }

      let keyName = keyMap[e.key] || e.key
      if (keyName.length === 1) keyName = keyName.toUpperCase()

      // Letters and digits require at least one modifier to avoid interfering with typing
      if (parts.length === 0 && !STANDALONE_KEYS.has(keyName)) return

      parts.push(keyName)
      const combo = parts.join('+')
      setPending(combo)

      // Auto-confirm after 1.5 seconds
      if (autoConfirmTimer.current) clearTimeout(autoConfirmTimer.current)
      autoConfirmTimer.current = setTimeout(() => {
        confirmHotkey(combo)
      }, 1500)
    },
    [confirmHotkey],
  )

  const handleKeyUp = useCallback(() => {
    setModifierHint(null)
  }, [])

  useEffect(() => {
    if (!recording) return
    window.addEventListener('keydown', handleKeyDown, true)
    window.addEventListener('keyup', handleKeyUp, true)
    return () => {
      window.removeEventListener('keydown', handleKeyDown, true)
      window.removeEventListener('keyup', handleKeyUp, true)
      if (autoConfirmTimer.current) clearTimeout(autoConfirmTimer.current)
    }
  }, [recording, handleKeyDown, handleKeyUp])

  const handleClick = () => {
    if (recording && pending) {
      // Confirm immediately on click
      if (autoConfirmTimer.current) clearTimeout(autoConfirmTimer.current)
      confirmHotkey(pending)
    } else if (recording) {
      // Cancel recording — re-register the old hotkey
      setRecording(false)
      setPending(null)
      setModifierHint(null)
      if (autoConfirmTimer.current) clearTimeout(autoConfirmTimer.current)
      resumeHotkey().catch(() => {})
    } else {
      // Start recording — unregister global shortcut so webview can capture keys
      pauseHotkey().catch(() => {})
      setRecording(true)
      setPending(null)
      setError(null)
    }
  }

  return (
    <div>
      <button
        onClick={handleClick}
        className={`w-full px-3 py-2.5 rounded-[10px] text-[13px] font-mono text-left border transition-colors cursor-pointer ${
          recording
            ? 'bg-bg-tertiary border-text-secondary text-text-primary ring-2 ring-text-secondary/20'
            : 'bg-bg-secondary border-transparent text-text-primary hover:border-border'
        }`}
      >
        {recording ? pending || modifierHint || t('settings.pressKeyCombination') : config.hotkey}
      </button>
      {recording && pending && (
        <p className="text-[11px] text-text-tertiary mt-1.5">{t('settings.clickToConfirm')}</p>
      )}
      {error && <p className="text-[11px] text-error mt-1.5">{error}</p>}
    </div>
  )
}

export function GeneralPane() {
  const config = useAppStore((s) => s.config)
  const savedConfig = useAppStore((s) => s.savedConfig)
  const updateConfig = useAppStore((s) => s.updateConfig)
  const { t } = useTranslation()

  const retentionDays = config.history_retention_days

  // A value written by a hand edit or a build offering different choices must stay
  // visible. Without this the select falls back to rendering its first option
  // ("Forever") while the backend keeps pruning at the stored value.
  const retentionOptions = useMemo(
    () =>
      RETENTION_OPTIONS.includes(retentionDays)
        ? RETENTION_OPTIONS
        : [...RETENTION_OPTIONS, retentionDays].sort((a, b) => a - b),
    [retentionDays],
  )

  // Narrowing the window deletes stored entries the moment Save is pressed, and
  // there is no undo — the same data behind the confirm-gated "Clear All History".
  // Compare against what is on disk, since that is what decides whether anything
  // actually gets deleted.
  //
  // The confirmation must go through ConfirmDialog, not `window.confirm` — the
  // latter returns falsy without displaying anything on macOS, which would
  // silently discard every narrowing change. See ConfirmDialog's doc comment.
  const [pendingRetention, setPendingRetention] = useState<number | null>(null)

  const handleRetentionChange = (value: number) => {
    const persisted = savedConfig?.history_retention_days ?? retentionDays
    const narrows = value !== 0 && (persisted === 0 || value < persisted)
    if (narrows) {
      setPendingRetention(value)
      return
    }
    updateConfig({ history_retention_days: value })
  }

  return (
    <div className="space-y-6">
      <Section title={t('settings.hotkey')}>
        <HotkeyRecorder />
        <div className="mt-3">
          <SegmentedControl
            options={[
              { value: 'hold', label: t('settings.holdToTalk') },
              { value: 'toggle', label: t('settings.toggleOnOff') },
            ]}
            value={config.hotkey_mode}
            onChange={(v) => updateConfig({ hotkey_mode: v as HotkeyMode })}
          />
        </div>
      </Section>

      <Section title={t('settings.maxRecordingDuration', 'Max Recording Duration')}>
        <div className="flex items-center gap-3">
          <input
            type="range"
            min={10}
            max={300}
            step={10}
            value={config.max_recording_seconds}
            onChange={(e) => updateConfig({ max_recording_seconds: Number(e.target.value) })}
            className="flex-1 accent-accent"
          />
          <span className="text-[13px] text-text-secondary font-mono w-12 text-right">
            {config.max_recording_seconds}s
          </span>
        </div>
      </Section>

      <Section title={t('settings.other')}>
        <div className="space-y-3">
          <Toggle
            checked={config.auto_start}
            onChange={(checked) => updateConfig({ auto_start: checked })}
            label={t('settings.launchAtStartup')}
          />
          <Toggle
            checked={config.capsule_auto_hide}
            onChange={(checked) => updateConfig({ capsule_auto_hide: checked })}
            label={t('settings.hideCapsuleWhenIdle')}
          />
        </div>
      </Section>

      <Section title={t('settings.history')}>
        <div className="space-y-3">
          <Toggle
            checked={config.history_enabled}
            onChange={(checked) => updateConfig({ history_enabled: checked })}
            label={t('settings.saveHistory')}
          />
          <div>
            <label
              htmlFor="history-retention"
              className="block text-[13px] text-text-primary mb-1.5"
            >
              {t('settings.keepHistoryFor')}
            </label>
            <select
              id="history-retention"
              value={retentionDays}
              onChange={(e) => handleRetentionChange(Number(e.target.value))}
              className="w-full px-3 py-2.5 bg-bg-secondary border border-border rounded-[10px] text-[13px] text-text-primary outline-none focus:border-border-focus transition-colors"
            >
              {retentionOptions.map((days) => (
                <option key={days} value={days}>
                  {days === 0
                    ? t('settings.retentionForever')
                    : t('settings.retentionDays', { days })}
                </option>
              ))}
            </select>
            <p className="text-[11px] text-text-tertiary mt-1.5">
              {retentionDays === 0
                ? t('settings.retentionHintForever')
                : t('settings.retentionHintDays', { days: retentionDays })}
            </p>
          </div>
        </div>
      </Section>

      <ConfirmDialog
        open={pendingRetention !== null}
        message={t('settings.retentionConfirm', { days: pendingRetention ?? 0 })}
        confirmLabel={t('common.delete')}
        destructive
        onConfirm={() => {
          if (pendingRetention !== null) {
            updateConfig({ history_retention_days: pendingRetention })
          }
          setPendingRetention(null)
        }}
        onCancel={() => setPendingRetention(null)}
      />
    </div>
  )
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div>
      <h3 className="text-[11px] font-medium text-text-tertiary uppercase tracking-wider mb-2.5">
        {title}
      </h3>
      {children}
    </div>
  )
}
