import { useEffect } from 'react'
import { useAppStore } from '../stores/appStore'
import { getCredentialStatus, setApiKey } from './tauri'

/**
 * Hand any typed API keys to the OS credential vault.
 *
 * Shared by the Settings save bar and onboarding — the only two places a key
 * can be entered. Rejections propagate: a key the credential store refused must
 * not be reported as saved, because the draft in the field is the only
 * remaining copy.
 *
 * Does not clear the drafts; the caller does that once the rest of its save
 * succeeded too.
 */
export async function writeKeyDrafts(): Promise<void> {
  const { config, keyDrafts } = useAppStore.getState()
  if (keyDrafts.stt !== null) {
    await setApiKey('stt', config.stt_provider, keyDrafts.stt)
  }
  if (keyDrafts.llm !== null) {
    await setApiKey('llm', config.llm_provider, keyDrafts.llm)
  }
}

/** Re-read whether the selected providers have a key saved. */
export async function refreshCredentialStatus(): Promise<void> {
  const { config, setCredentialStatus } = useAppStore.getState()
  setCredentialStatus(await getCredentialStatus(config.stt_provider, config.llm_provider))
}

/**
 * Keep the "a key is saved" flags in step with the selected providers.
 *
 * Credentials are stored per provider, so switching provider changes which
 * entry the panes are asking about — a key saved for Deepgram must not make the
 * AssemblyAI field claim it already has one.
 */
export function useCredentialStatusSync(): void {
  const sttProvider = useAppStore((s) => s.config.stt_provider)
  const llmProvider = useAppStore((s) => s.config.llm_provider)
  const configLoaded = useAppStore((s) => s.configLoaded)
  const setCredentialStatus = useAppStore((s) => s.setCredentialStatus)

  useEffect(() => {
    // Before the config lands, the providers are still the defaults; asking
    // about those would flash the wrong state into the panes.
    if (!configLoaded) return
    let cancelled = false
    getCredentialStatus(sttProvider, llmProvider)
      .then((status) => {
        if (!cancelled) setCredentialStatus(status)
      })
      .catch((e) => {
        console.error('Failed to read credential status:', e)
      })
    return () => {
      cancelled = true
    }
  }, [configLoaded, sttProvider, llmProvider, setCredentialStatus])
}
