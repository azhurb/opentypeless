import { useTranslation } from 'react-i18next'
import { useAppStore } from '../../stores/appStore'
import { STT_PROVIDERS, LANGUAGES } from '../../lib/constants'
import { benchSttConnection } from '../../lib/tauri'
import { useApiKeyField } from '../../hooks/useApiKeyField'
import { FormField } from './shared/FormField'
import { CheckCircle2, XCircle, Loader2, Check } from 'lucide-react'

export function SttPane() {
  const config = useAppStore((s) => s.config)
  const updateConfig = useAppStore((s) => s.updateConfig)
  const sttTestStatus = useAppStore((s) => s.sttTestStatus)
  const setSttTestStatus = useAppStore((s) => s.setSttTestStatus)
  const sttLatencyMs = useAppStore((s) => s.sttLatencyMs)
  const setSttLatencyMs = useAppStore((s) => s.setSttLatencyMs)
  const apiKey = useApiKeyField('stt')
  const { t } = useTranslation()

  const handleTest = async () => {
    setSttTestStatus('testing')
    setSttLatencyMs(null)
    try {
      const ms = await benchSttConnection(apiKey.probeKey, config.stt_provider)
      console.log('[STT Test] Received latency:', ms, 'type:', typeof ms)
      setSttLatencyMs(ms)
      setSttTestStatus('success')
    } catch (err) {
      console.error('[STT Test] Error:', err)
      setSttTestStatus('error')
    }
  }

  return (
    <div className="space-y-5">
      <FormField label={t('settings.provider')}>
        <select
          value={config.stt_provider}
          onChange={(e) => {
            updateConfig({ stt_provider: e.target.value as typeof config.stt_provider })
            setSttTestStatus('idle')
            setSttLatencyMs(null)
          }}
          className="w-full px-3 py-2.5 bg-bg-secondary border border-border rounded-[10px] text-[13px] text-text-primary outline-none focus:border-border-focus transition-colors"
        >
          {STT_PROVIDERS.map((p) => (
            <option key={p.value} value={p.value}>
              {p.label}
            </option>
          ))}
        </select>
      </FormField>

      <FormField label={t('settings.apiKey')}>
        <div className="flex gap-2">
          <input
            type="password"
            value={apiKey.value}
            onChange={(e) => {
              apiKey.onChange(e.target.value)
              setSttTestStatus('idle')
              setSttLatencyMs(null)
            }}
            placeholder={
              apiKey.isUnreadable
                ? t('settings.apiKeyUnreadable')
                : apiKey.hasSavedKey
                  ? t('settings.apiKeySaved')
                  : t('settings.enterApiKey')
            }
            className="flex-1 px-3 py-2.5 bg-bg-secondary border border-border rounded-[10px] text-[13px] text-text-primary outline-none focus:border-border-focus transition-colors"
          />
          <button
            onClick={handleTest}
            disabled={!apiKey.canTest || sttTestStatus === 'testing'}
            className="px-4 py-2.5 bg-accent text-white rounded-[10px] text-[13px] border-none cursor-pointer hover:bg-accent-hover disabled:opacity-40 disabled:cursor-not-allowed transition-colors flex items-center gap-1.5"
          >
            {sttTestStatus === 'testing' && <Loader2 size={14} className="animate-spin" />}
            {t('settings.test')}
          </button>
        </div>
        {sttTestStatus === 'success' && (
          <p className="flex items-center gap-1 text-[12px] text-success mt-2">
            <CheckCircle2 size={13} />{' '}
            {sttLatencyMs !== null ? `${sttLatencyMs}ms` : t('settings.connectionSuccess')}
          </p>
        )}
        {sttTestStatus === 'error' && (
          <p className="flex items-center gap-1 text-[12px] text-error mt-2">
            <XCircle size={13} /> {t('settings.connectionFailed')}
          </p>
        )}
        {apiKey.isUnreadable && (
          <p className="flex items-start gap-1 text-[12px] text-warning mt-2">
            <XCircle size={13} className="flex-shrink-0 mt-0.5" />
            {t('settings.apiKeyUnreadableHint')}
          </p>
        )}
        <div className="flex items-center justify-between gap-3 mt-1.5">
          <p className="text-[11px] text-text-tertiary">{t('settings.storedLocally')}</p>
          {apiKey.hasSavedKey && (
            <button
              type="button"
              onClick={() => {
                apiKey.clear()
                setSttTestStatus('idle')
                setSttLatencyMs(null)
              }}
              className="flex-shrink-0 text-[11px] text-text-tertiary hover:text-error bg-transparent border-none cursor-pointer p-0 transition-colors"
            >
              {t('settings.apiKeyRemove')}
            </button>
          )}
        </div>
      </FormField>

      <FormField label={t('settings.sttLanguages')}>
        <div className="flex flex-wrap gap-1.5">
          {LANGUAGES.map((l) => {
            const selected = config.stt_languages.includes(l.value)
            return (
              <button
                key={l.value}
                type="button"
                aria-pressed={selected}
                onClick={() => {
                  const next = selected
                    ? config.stt_languages.filter((c) => c !== l.value)
                    : [...config.stt_languages, l.value]
                  updateConfig({ stt_languages: next })
                }}
                className={`inline-flex items-center gap-1 px-3 py-1.5 rounded-full text-[12px] border transition-colors cursor-pointer ${
                  selected
                    ? 'bg-accent/10 border-accent text-accent'
                    : 'bg-bg-secondary border-border text-text-primary hover:border-text-tertiary'
                }`}
              >
                {selected && <Check size={12} />}
                {l.label}
              </button>
            )
          })}
        </div>
        <p className="text-[11px] text-text-tertiary mt-2">
          {config.stt_languages.length === 0
            ? t('settings.sttLanguagesAutoHint')
            : config.stt_languages.length === 1
              ? t('settings.sttLanguagesSingleHint')
              : t('settings.sttLanguagesMultiHint')}
        </p>
      </FormField>
    </div>
  )
}
