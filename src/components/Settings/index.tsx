import { useState } from 'react'
import { AnimatePresence, motion } from 'framer-motion'
import { useTranslation } from 'react-i18next'
import { SettingsSidebar, type PaneId } from './SettingsSidebar'
import { GeneralPane } from './GeneralPane'
import { SttPane } from './SttPane'
import { LlmPane } from './LlmPane'
import { DictionaryPane } from './DictionaryPane'
import { AboutPane } from './AboutPane'
import { DirtyBar, useDirtyConfig } from './shared/DirtyBar'

const paneTitleKeys: Record<PaneId, string> = {
  general: 'settings.general',
  stt: 'settings.speechRecognition',
  llm: 'settings.aiPolish',
  dictionary: 'settings.dictionary',
  about: 'settings.about',
}

export function Settings() {
  const [activePane, setActivePane] = useState<PaneId>('general')
  const isDirty = useDirtyConfig()
  const { t } = useTranslation()

  // No snapshot on mount. `savedConfig` already tracks what Rust has on disk
  // (seeded by the initial load, refreshed by `setConfig` on `config:changed`).
  // Re-snapshotting here used to silently adopt *unsaved* edits as the baseline,
  // so leaving Settings dirty and coming back made the "Unsaved changes" bar
  // disappear while the edits were never persisted — visible with the history
  // toggle, where the History pane would then claim saving was off while the
  // backend kept recording.

  return (
    <div className="w-full h-full bg-bg-primary text-text-primary flex flex-col">
      <div className="flex-1 flex min-h-0">
        {/* Sidebar */}
        <SettingsSidebar activePane={activePane} onSelect={setActivePane} />

        {/* Content */}
        <div className="flex-1 flex flex-col min-w-0">
          {/* Title bar */}
          <div className="flex items-center justify-between px-6 pt-4 pb-3 border-b border-border bg-bg-primary/50">
            <h2 className="text-[15px] font-medium">{t(paneTitleKeys[activePane])}</h2>
          </div>

          {/* Pane content */}
          <div className="flex-1 overflow-y-auto px-6 py-5">
            <AnimatePresence mode="sync">
              <motion.div
                key={activePane}
                className="w-full"
                initial={{ opacity: 0, y: 6 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: -6 }}
                transition={{ duration: 0.1, ease: 'easeOut' }}
              >
                {activePane === 'general' && <GeneralPane />}
                {activePane === 'stt' && <SttPane />}
                {activePane === 'llm' && <LlmPane />}
                {activePane === 'dictionary' && <DictionaryPane />}
                {activePane === 'about' && <AboutPane />}
              </motion.div>
            </AnimatePresence>
          </div>
        </div>
      </div>

      {/* Dirty bar */}
      <AnimatePresence>{isDirty && <DirtyBar />}</AnimatePresence>
    </div>
  )
}
