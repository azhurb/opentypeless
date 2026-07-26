import { useCallback } from 'react'
import { useAppStore } from '../stores/appStore'
import type { CredentialNamespace } from '../lib/tauri'

/**
 * The state an API key input needs, given that the webview can never read the
 * key back.
 *
 * The field is genuinely empty when a key is saved — it shows a placeholder,
 * not a masked value. A fake value would have to be compared against the real
 * config by the unsaved-changes bar, which is how the 0.5.0 phantom-dirty bug
 * happened (see CHANGELOG). Here `draft === null` means "untouched", so an
 * untouched pane is unambiguously clean.
 */
export interface ApiKeyField {
  /** Bind to `<input value>`. Empty while a saved key sits untouched. */
  value: string
  /** A key is in the vault and the user has not started replacing it. */
  hasSavedKey: boolean
  onChange: (next: string) => void
  /** Drop the saved key. Takes effect on Save, like every other setting. */
  clear: () => void
  /** Something is testable: either a typed candidate or a stored key. */
  canTest: boolean
  /**
   * What to hand the test/bench commands: the typed candidate, or `null` to
   * mean "use the stored key". Never send a placeholder.
   */
  probeKey: string | null
}

export function useApiKeyField(namespace: CredentialNamespace): ApiKeyField {
  const draft = useAppStore((s) => s.keyDrafts[namespace])
  const setKeyDraft = useAppStore((s) => s.setKeyDraft)
  const savedInVault = useAppStore((s) => s.credentialStatus[namespace])

  const onChange = useCallback(
    (next: string) => setKeyDraft(namespace, next),
    [namespace, setKeyDraft],
  )

  // An empty-string draft is a deliberate "remove this key", which is why it
  // has to stay distinct from `null`.
  const clear = useCallback(() => setKeyDraft(namespace, ''), [namespace, setKeyDraft])

  const hasSavedKey = savedInVault && draft === null

  return {
    value: draft ?? '',
    hasSavedKey,
    onChange,
    clear,
    canTest: draft !== null ? draft.length > 0 : savedInVault,
    probeKey: draft,
  }
}
