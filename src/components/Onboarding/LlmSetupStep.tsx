import { useState, useEffect, useCallback, useRef } from 'react'
import { useAppStore } from '../../stores/appStore'
import { LLM_PROVIDERS, LLM_DEFAULT_CONFIG } from '../../lib/constants'
import { testLlmConnection, fetchLlmModels } from '../../lib/tauri'
import { useApiKeyField } from '../../hooks/useApiKeyField'
import { CheckCircle2, XCircle, Loader2, RefreshCw } from 'lucide-react'

export function LlmSetupStep() {
  const config = useAppStore((s) => s.config)
  const updateConfig = useAppStore((s) => s.updateConfig)
  const llmTestStatus = useAppStore((s) => s.llmTestStatus)
  const setLlmTestStatus = useAppStore((s) => s.setLlmTestStatus)
  const apiKey = useApiKeyField('llm')

  const [models, setModels] = useState<string[]>([])
  const [fetchingModels, setFetchingModels] = useState(false)
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  const doFetchModels = useCallback(
    async (probeKey: string | null, provider: string, baseUrl: string) => {
      if (!baseUrl) return
      setFetchingModels(true)
      try {
        const list = await fetchLlmModels(probeKey, provider, baseUrl)
        setModels(list)
      } catch {
        setModels([])
      } finally {
        setFetchingModels(false)
      }
    },
    [],
  )

  // Auto-fetch when the key changes (debounced). The key is a draft during
  // onboarding — nothing is saved yet — so this watches the draft, not config.
  const { probeKey, canTest } = apiKey
  useEffect(() => {
    if (debounceRef.current) clearTimeout(debounceRef.current)
    if (!canTest || !config.llm_base_url) return
    debounceRef.current = setTimeout(() => {
      doFetchModels(probeKey, config.llm_provider, config.llm_base_url)
    }, 500)
    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current)
    }
  }, [probeKey, canTest, config.llm_provider, config.llm_base_url, doFetchModels])

  const handleTest = async () => {
    setLlmTestStatus('testing')
    try {
      const ok = await testLlmConnection(
        apiKey.probeKey,
        config.llm_provider,
        config.llm_base_url,
        config.llm_model,
      )
      setLlmTestStatus(ok ? 'success' : 'error')
    } catch {
      setLlmTestStatus('error')
    }
  }

  return (
    <div className="space-y-5">
      <Field label="AI Polish Service">
        <select
          value={config.llm_provider}
          onChange={(e) => {
            const provider = e.target.value as typeof config.llm_provider
            const defaults = LLM_DEFAULT_CONFIG[provider]
            updateConfig({
              llm_provider: provider,
              llm_base_url: defaults?.baseUrl ?? config.llm_base_url,
              llm_model: defaults?.model ?? config.llm_model,
            })
            setLlmTestStatus('idle')
            setModels([])
          }}
          className="w-full px-3 py-2.5 bg-bg-secondary border border-border rounded-[10px] text-[13px] text-text-primary outline-none focus:border-border-focus transition-colors"
        >
          {LLM_PROVIDERS.map((p) => (
            <option key={p.value} value={p.value}>
              {p.label}
            </option>
          ))}
        </select>
      </Field>

      <Field label="API Key">
        <div className="flex gap-2">
          <input
            type="password"
            value={apiKey.value}
            onChange={(e) => {
              apiKey.onChange(e.target.value)
              setLlmTestStatus('idle')
            }}
            placeholder={apiKey.hasSavedKey ? 'Saved in your system keychain' : 'Enter API Key...'}
            className="flex-1 px-3 py-2.5 bg-bg-secondary border border-border rounded-[10px] text-[13px] text-text-primary outline-none focus:border-border-focus transition-colors"
          />
          <button
            onClick={handleTest}
            disabled={!apiKey.canTest || llmTestStatus === 'testing'}
            className="px-4 py-2.5 bg-accent text-white rounded-[10px] text-[13px] border-none cursor-pointer hover:bg-accent-hover disabled:opacity-40 disabled:cursor-not-allowed transition-colors flex items-center gap-1.5"
          >
            {llmTestStatus === 'testing' && <Loader2 size={14} className="animate-spin" />}
            Test
          </button>
        </div>
        <TestStatusHint status={llmTestStatus} />
      </Field>

      <Field label="Model">
        <div className="flex gap-2">
          <div className="relative flex-1">
            <input
              list="onboarding-llm-model-list"
              value={config.llm_model}
              onChange={(e) => updateConfig({ llm_model: e.target.value })}
              placeholder="glm-4.7"
              className="w-full px-3 py-2.5 bg-bg-secondary border border-border rounded-[10px] text-[13px] text-text-primary outline-none focus:border-border-focus transition-colors"
            />
            <datalist id="onboarding-llm-model-list">
              {models.map((m) => (
                <option key={m} value={m} />
              ))}
            </datalist>
          </div>
          <button
            onClick={() => doFetchModels(apiKey.probeKey, config.llm_provider, config.llm_base_url)}
            disabled={fetchingModels || !config.llm_base_url}
            className="px-3 py-2.5 bg-bg-secondary border border-border rounded-[10px] text-[13px] text-text-secondary cursor-pointer hover:border-border-focus disabled:opacity-40 disabled:cursor-not-allowed transition-colors flex items-center gap-1.5"
            title="Fetch available models"
          >
            <RefreshCw size={14} className={fetchingModels ? 'animate-spin' : ''} />
          </button>
        </div>
        {models.length > 0 && (
          <p className="text-[11px] text-text-tertiary mt-1">{models.length} models available</p>
        )}
      </Field>

      <Field label="Base URL">
        <input
          value={config.llm_base_url}
          onChange={(e) => updateConfig({ llm_base_url: e.target.value })}
          placeholder="https://open.bigmodel.cn/api/paas/v4"
          className="w-full px-3 py-2.5 bg-bg-secondary border border-border rounded-[10px] text-[13px] text-text-primary outline-none focus:border-border-focus transition-colors"
        />
      </Field>
    </div>
  )
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <label className="block text-[13px] font-medium text-text-secondary mb-2">{label}</label>
      {children}
    </div>
  )
}

function TestStatusHint({ status }: { status: string }) {
  if (status === 'success') {
    return (
      <p className="flex items-center gap-1 text-[12px] text-success mt-2">
        <CheckCircle2 size={13} /> Connection successful
      </p>
    )
  }
  if (status === 'error') {
    return (
      <p className="flex items-center gap-1 text-[12px] text-error mt-2">
        <XCircle size={13} /> Connection failed, please check your config
      </p>
    )
  }
  return null
}
