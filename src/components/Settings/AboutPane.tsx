import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import i18n from '../../i18n'
import { ExternalLink } from 'lucide-react'
import { openUrl } from '@tauri-apps/plugin-opener'
import { getVersion } from '@tauri-apps/api/app'
import { useAppStore } from '../../stores/appStore'
import { APP_NAME, APP_REPO_URL } from '../../lib/constants'

const UI_LANGUAGES = [
  { code: 'en', label: 'English', native: 'English' },
  { code: 'zh', label: 'Chinese', native: '中文' },
] as const

export function AboutPane() {
  const { t } = useTranslation()
  const config = useAppStore((s) => s.config)
  const updateConfig = useAppStore((s) => s.updateConfig)

  const currentLang = config.ui_language || i18n.language || 'en'

  // Read the version from the bundle rather than a checked-in constant. The
  // release workflow rewrites tauri.conf.json from the tag, so this is the only
  // value that tracks the actual release; a constant in the repo stays at
  // 0.1.0 because the workflow deliberately never commits version bumps.
  const [version, setVersion] = useState('')
  useEffect(() => {
    getVersion()
      // No IPC outside the Tauri shell (e.g. vitest, a browser preview). Show
      // nothing rather than a hardcoded number that would be wrong again.
      .then((v) => setVersion(`v${v}`))
      .catch(() => setVersion(''))
  }, [])

  const handleSelectLanguage = (code: string) => {
    i18n.changeLanguage(code)
    localStorage.setItem('ui_language', code)
    updateConfig({ ui_language: code })
  }

  return (
    <div className="space-y-5 text-[13px]">
      {/* Header */}
      <div className="text-center py-6">
        <h2 className="text-[22px] font-semibold text-text-primary">{APP_NAME}</h2>
        <p className="text-text-secondary mt-1 text-[13px]">{version}</p>
      </div>

      <p className="text-text-secondary leading-relaxed">{t('settings.aboutDescription')}</p>

      {/* Language */}
      <SectionCard title={t('settings.language')}>
        <div className="grid grid-cols-2 gap-3 p-3">
          {UI_LANGUAGES.map((lang) => (
            <button
              key={lang.code}
              onClick={() => handleSelectLanguage(lang.code)}
              className={`px-4 py-3 rounded-[8px] text-[13px] border cursor-pointer transition-all ${
                currentLang === lang.code
                  ? 'bg-accent/10 border-accent text-accent font-medium'
                  : 'bg-bg-secondary border-border text-text-primary hover:border-text-tertiary'
              }`}
            >
              <div className="font-medium">{lang.native}</div>
              <div className="text-[11px] text-text-tertiary mt-0.5">{lang.label}</div>
            </button>
          ))}
        </div>
      </SectionCard>

      {/* Open Source */}
      <SectionCard title={t('settings.openSource')}>
        <InfoRow label={t('settings.license')} value={t('settings.mit')} />
        <LinkRow label={t('settings.github')} url={APP_REPO_URL} linkText={t('settings.view')} />
        <InfoRow label={t('settings.framework')} value={t('settings.tauriReact')} />
      </SectionCard>
    </div>
  )
}

function SectionCard({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="border border-border rounded-[10px] overflow-hidden">
      <div className="px-3 py-2.5 bg-bg-secondary/50 border-b border-border">
        <h3 className="text-[13px] font-medium text-text-primary">{title}</h3>
      </div>
      {children}
    </div>
  )
}

function InfoRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex justify-between px-3 py-2.5 border-b border-border last:border-b-0">
      <span className="text-text-secondary">{label}</span>
      <span className="text-text-primary">{value}</span>
    </div>
  )
}

function LinkRow({ label, url, linkText }: { label: string; url: string; linkText: string }) {
  return (
    <button
      onClick={() => openUrl(url)}
      className="flex justify-between items-center w-full px-3 py-2.5 border-b border-border last:border-b-0 bg-transparent border-x-0 border-t-0 cursor-pointer text-[13px]"
    >
      <span className="text-text-secondary">{label}</span>
      <span className="text-accent flex items-center gap-1">
        {linkText} <ExternalLink size={12} />
      </span>
    </button>
  )
}
