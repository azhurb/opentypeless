//! Provider API keys, stored in the OS credential vault.
//!
//! Keys used to live in `settings.json` as plain strings, which for a BYOK app
//! is the widest gap between what the README claims and what the app does. They
//! now live in the macOS Keychain / Windows Credential Manager / Linux Secret
//! Service, and [`migrate_legacy_config_secrets`] moves any plaintext left over
//! from an older install on first launch.
//!
//! Secrets are keyed by `(namespace, provider)`, not by namespace alone, so
//! switching STT provider and switching back remembers the earlier key instead
//! of overwriting it.
//!
//! Everything here goes through the [`CredentialVault`] trait. That indirection
//! exists for one reason: `cargo test` runs on three OSes in CI, and a test that
//! touches the real vault either prompts for authorization or fails on a
//! headless runner. Tests use [`MemoryVault`]; only the app constructs a
//! [`SystemCredentialVault`].

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Vault service name. Matches the bundle identifier in `tauri.conf.json` so
/// the entries are attributable in Keychain Access / Credential Manager.
const SERVICE_NAME: &str = "com.opentypeless.app";

/// Stamped onto every stored payload. Nothing reads a version other than 1 yet;
/// it is here so a future format change can be detected rather than guessed at,
/// since a vault entry outlives any single app version.
const STORED_CREDENTIAL_VERSION: u8 = 1;

/// Namespace for speech-to-text provider keys.
pub const STT_NAMESPACE: &str = "stt";
/// Namespace for LLM provider keys.
pub const LLM_NAMESPACE: &str = "llm";

/// The JSON envelope actually written to the vault. Storing a struct rather
/// than the bare secret costs a few bytes and buys the version stamp.
#[derive(Debug, Serialize, Deserialize)]
struct StoredCredential {
    version: u8,
    secret: String,
}

/// Identifies one secret. `namespace` is `stt` or `llm`; `provider` is the
/// provider id as it appears in `AppConfig` (`deepgram`, `openrouter`, …).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CredentialId {
    pub namespace: String,
    pub provider: String,
}

impl CredentialId {
    pub fn new(namespace: impl Into<String>, provider: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            provider: provider.into(),
        }
    }

    pub fn stt(provider: impl Into<String>) -> Self {
        Self::new(STT_NAMESPACE, provider)
    }

    pub fn llm(provider: impl Into<String>) -> Self {
        Self::new(LLM_NAMESPACE, provider)
    }

    /// The account name under `SERVICE_NAME`. Stable — changing this format
    /// orphans every entry a previous version wrote.
    pub fn account(&self) -> String {
        format!("{}:{}", self.namespace, self.provider)
    }
}

/// Read/write/delete against a credential store.
///
/// One trait rather than the reader/writer/remover split upstream uses: the
/// point of the abstraction is that tests can substitute [`MemoryVault`], and
/// three traits over three methods on a single implementor does not buy more of
/// that than one does.
pub trait CredentialVault: Send + Sync {
    /// `Ok(None)` means "no entry", which is distinct from `Err`, which means
    /// the vault itself could not be reached. Callers must not collapse the two
    /// — a locked keyring reported as "no key" turns into the pipeline's
    /// misleading "API key is not configured".
    fn read(&self, id: &CredentialId) -> Result<Option<String>>;
    fn write(&self, id: &CredentialId, secret: &str) -> Result<()>;
    /// Deleting an entry that does not exist is `Ok(())`.
    fn delete(&self, id: &CredentialId) -> Result<()>;
}

/// The vault as held in Tauri managed state and by [`crate::storage::ConfigManager`].
pub type SharedVault = std::sync::Arc<dyn CredentialVault>;

/// Remembers secrets it has already read, so one app session touches the OS
/// credential store about twice instead of twice per dictation.
///
/// This is a usability fix with teeth on macOS. A Keychain prompt offers Deny /
/// Allow / **Always** Allow, and plain "Allow" grants exactly one access — so a
/// user who picks it over Always Allow was being re-prompted on every single
/// dictation (the pipeline reads the STT key and the LLM key each time). Users
/// reasonably read repeated credential prompts as something malicious.
///
/// Only *successful* reads are cached. An error is never cached, so a locked
/// keychain keeps reporting itself rather than being remembered as a failure
/// for the session; a miss is never cached either, so a key added out of band
/// is still picked up.
///
/// The tradeoff: a resolved secret stays in process memory for the session
/// rather than only for the duration of a request. It was already in memory
/// whenever a request was in flight, and it is never written anywhere or handed
/// to the webview.
pub struct CachingVault {
    inner: SharedVault,
    cache: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

impl CachingVault {
    pub fn new(inner: SharedVault) -> Self {
        Self {
            inner,
            cache: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }
}

impl CredentialVault for CachingVault {
    fn read(&self, id: &CredentialId) -> Result<Option<String>> {
        if let Some(hit) = self
            .cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id.account())
        {
            return Ok(Some(hit.clone()));
        }
        let fresh = self.inner.read(id)?;
        if let Some(secret) = &fresh {
            self.cache
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(id.account(), secret.clone());
        }
        Ok(fresh)
    }

    fn write(&self, id: &CredentialId, secret: &str) -> Result<()> {
        self.inner.write(id, secret)?;
        // Only after the store accepted it — caching first would serve a secret
        // that isn't actually persisted.
        self.cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id.account(), secret.to_string());
        Ok(())
    }

    fn delete(&self, id: &CredentialId) -> Result<()> {
        self.inner.delete(id)?;
        self.cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id.account());
        Ok(())
    }
}

/// The real vault: Keychain on macOS, Credential Manager on Windows, Secret
/// Service on Linux (see the per-platform `keyring` features in `Cargo.toml`).
pub struct SystemCredentialVault;

impl SystemCredentialVault {
    pub fn new() -> Self {
        Self
    }

    fn entry(id: &CredentialId) -> Result<keyring::Entry> {
        keyring::Entry::new(SERVICE_NAME, &id.account())
            .with_context(|| format!("failed to open vault entry for {}", id.account()))
    }
}

impl Default for SystemCredentialVault {
    fn default() -> Self {
        Self::new()
    }
}

impl CredentialVault for SystemCredentialVault {
    fn read(&self, id: &CredentialId) -> Result<Option<String>> {
        let entry = Self::entry(id)?;
        match entry.get_password() {
            Ok(raw) => Ok(Some(decode_secret(&raw))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(anyhow::Error::new(e)
                .context(format!("failed to read vault entry {}", id.account()))),
        }
    }

    fn write(&self, id: &CredentialId, secret: &str) -> Result<()> {
        let payload = serde_json::to_string(&StoredCredential {
            version: STORED_CREDENTIAL_VERSION,
            secret: secret.to_string(),
        })?;
        Self::entry(id)?
            .set_password(&payload)
            .with_context(|| format!("failed to write vault entry {}", id.account()))
    }

    fn delete(&self, id: &CredentialId) -> Result<()> {
        match Self::entry(id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(anyhow::Error::new(e)
                .context(format!("failed to delete vault entry {}", id.account()))),
        }
    }
}

/// Unwrap the stored envelope, tolerating a bare secret.
///
/// Nothing in a shipped version ever wrote a bare secret, but a vault entry can
/// also be created by hand, and refusing to read one would look to the user
/// like the key silently vanished.
fn decode_secret(raw: &str) -> String {
    match serde_json::from_str::<StoredCredential>(raw) {
        Ok(stored) => stored.secret,
        Err(_) => raw.to_string(),
    }
}

// ─── Legacy plaintext migration ───

/// What [`migrate_legacy_config_secrets`] did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct MigrationOutcome {
    /// The raw config value was mutated and the caller must persist it.
    pub config_mutated: bool,
    /// Accounts successfully moved into the vault.
    pub migrated: Vec<String>,
    /// Accounts whose vault write failed. Their plaintext is deliberately still
    /// in the config, and the migration will retry on the next launch.
    pub failed: Vec<String>,
}

/// Legacy `AppConfig` field names, and the provider field each key belongs to.
const LEGACY_SECRETS: [(&str, &str, &str); 2] = [
    ("stt_api_key", "stt_provider", STT_NAMESPACE),
    ("llm_api_key", "llm_provider", LLM_NAMESPACE),
];

/// The plaintext fields this migration consumes. `ConfigManager::save` needs
/// the same list to carry forward a key the vault would not accept.
pub const LEGACY_SECRET_FIELDS: [&str; 2] = ["stt_api_key", "llm_api_key"];

/// Move plaintext API keys out of a raw `app_config` JSON value and into the
/// vault, filing each under the provider that was selected when it was saved.
///
/// The ordering is the whole point: the plaintext field is removed **only after
/// the vault write returns `Ok`**. A vault that is locked, unavailable, or
/// denied leaves the config exactly as it was, so the user keeps a working key
/// and the migration retries next launch. Clearing first would destroy the only
/// copy of a secret the user may not have written down anywhere.
///
/// Idempotent: a config with no legacy fields is untouched, and a field that is
/// empty (or already present in the vault) is dropped without a write.
pub fn migrate_legacy_config_secrets(
    vault: &dyn CredentialVault,
    value: &mut serde_json::Value,
) -> MigrationOutcome {
    let mut outcome = MigrationOutcome::default();
    let Some(obj) = value.as_object_mut() else {
        return outcome;
    };

    for (key_field, provider_field, namespace) in LEGACY_SECRETS {
        if !obj.contains_key(key_field) {
            continue;
        }

        let secret = obj
            .get(key_field)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        // An empty (or non-string) legacy field holds nothing worth keeping, so
        // it can go without touching the vault at all.
        if secret.is_empty() {
            obj.remove(key_field);
            outcome.config_mutated = true;
            continue;
        }

        let provider = obj
            .get(provider_field)
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if provider.is_empty() {
            // No provider to file the key under. Leaving the plaintext in place
            // is the safe direction: the key is still usable, and once the user
            // picks a provider the next launch migrates it.
            tracing::warn!(
                "cannot migrate {}: {} is missing, leaving plaintext in place",
                key_field,
                provider_field
            );
            outcome.failed.push(key_field.to_string());
            continue;
        }

        let id = CredentialId::new(namespace, provider);

        // Already vaulted — the user set a key through the new path before this
        // migration got a chance to run. Their vault entry wins; the stale
        // plaintext is what we are here to delete.
        match vault.read(&id) {
            Ok(Some(_)) => {
                obj.remove(key_field);
                outcome.config_mutated = true;
                continue;
            }
            Ok(None) => {}
            Err(e) => {
                tracing::error!(
                    "vault unreachable while migrating {}, keeping plaintext: {:#}",
                    id.account(),
                    e
                );
                outcome.failed.push(id.account());
                continue;
            }
        }

        match vault.write(&id, &secret) {
            Ok(()) => {
                obj.remove(key_field);
                outcome.config_mutated = true;
                outcome.migrated.push(id.account());
            }
            Err(e) => {
                tracing::error!(
                    "failed to move {} into the vault, keeping plaintext: {:#}",
                    id.account(),
                    e
                );
                outcome.failed.push(id.account());
            }
        }
    }

    outcome
}

// ─── Test double ───

/// In-memory [`CredentialVault`] for tests.
///
/// Public and not `#[cfg(test)]` so integration tests and other modules' test
/// modules can use it. Nothing in the app constructs one.
#[derive(Default)]
pub struct MemoryVault {
    entries: std::sync::Mutex<std::collections::HashMap<String, String>>,
    /// When set, every operation fails with this message — used to exercise the
    /// "vault write failed, keep the plaintext" path.
    fail_with: Option<String>,
    /// Counts `read` calls that actually reached this vault, so tests can prove
    /// [`CachingVault`] is keeping them away from the OS.
    reads: std::sync::atomic::AtomicUsize,
}

impl MemoryVault {
    pub fn new() -> Self {
        Self::default()
    }

    /// A vault where every operation errors.
    pub fn failing(message: impl Into<String>) -> Self {
        Self {
            fail_with: Some(message.into()),
            ..Self::default()
        }
    }

    /// How many reads reached this vault.
    pub fn read_count(&self) -> usize {
        self.reads.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn guard(&self) -> Result<()> {
        match &self.fail_with {
            Some(message) => Err(anyhow::anyhow!("{}", message)),
            None => Ok(()),
        }
    }
}

impl CredentialVault for MemoryVault {
    fn read(&self, id: &CredentialId) -> Result<Option<String>> {
        self.reads
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.guard()?;
        Ok(self
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id.account())
            .map(|raw| decode_secret(raw)))
    }

    fn write(&self, id: &CredentialId, secret: &str) -> Result<()> {
        self.guard()?;
        let payload = serde_json::to_string(&StoredCredential {
            version: STORED_CREDENTIAL_VERSION,
            secret: secret.to_string(),
        })?;
        self.entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id.account(), payload);
        Ok(())
    }

    fn delete(&self, id: &CredentialId) -> Result<()> {
        self.guard()?;
        self.entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id.account());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn account_is_namespace_and_provider() {
        assert_eq!(CredentialId::stt("deepgram").account(), "stt:deepgram");
        assert_eq!(CredentialId::llm("openrouter").account(), "llm:openrouter");
    }

    #[test]
    fn write_then_read_round_trips() {
        let vault = MemoryVault::new();
        let id = CredentialId::stt("deepgram");
        vault.write(&id, "sk-secret").unwrap();
        assert_eq!(vault.read(&id).unwrap(), Some("sk-secret".to_string()));
    }

    #[test]
    fn missing_entry_reads_as_none_not_error() {
        let vault = MemoryVault::new();
        assert_eq!(vault.read(&CredentialId::stt("deepgram")).unwrap(), None);
    }

    #[test]
    fn providers_do_not_share_a_slot() {
        // The reason credentials are keyed per provider: switching provider and
        // switching back used to overwrite the first key.
        let vault = MemoryVault::new();
        vault
            .write(&CredentialId::stt("deepgram"), "dg-key")
            .unwrap();
        vault
            .write(&CredentialId::stt("assemblyai"), "aai-key")
            .unwrap();
        assert_eq!(
            vault.read(&CredentialId::stt("deepgram")).unwrap(),
            Some("dg-key".to_string())
        );
        assert_eq!(
            vault.read(&CredentialId::stt("assemblyai")).unwrap(),
            Some("aai-key".to_string())
        );
    }

    #[test]
    fn namespaces_do_not_share_a_slot() {
        // `siliconflow` is both an STT and an LLM provider id.
        let vault = MemoryVault::new();
        vault
            .write(&CredentialId::stt("siliconflow"), "stt-key")
            .unwrap();
        vault
            .write(&CredentialId::llm("siliconflow"), "llm-key")
            .unwrap();
        assert_eq!(
            vault.read(&CredentialId::stt("siliconflow")).unwrap(),
            Some("stt-key".to_string())
        );
        assert_eq!(
            vault.read(&CredentialId::llm("siliconflow")).unwrap(),
            Some("llm-key".to_string())
        );
    }

    #[test]
    fn delete_removes_the_entry_and_is_idempotent() {
        let vault = MemoryVault::new();
        let id = CredentialId::llm("openrouter");
        vault.write(&id, "sk-secret").unwrap();
        vault.delete(&id).unwrap();
        assert_eq!(vault.read(&id).unwrap(), None);
        vault.delete(&id).unwrap();
    }

    #[test]
    fn stored_payload_is_versioned() {
        let stored: StoredCredential = serde_json::from_str(
            &serde_json::to_string(&StoredCredential {
                version: STORED_CREDENTIAL_VERSION,
                secret: "sk".to_string(),
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(stored.version, 1);
        assert_eq!(stored.secret, "sk");
    }

    #[test]
    fn hand_written_bare_secret_is_readable() {
        assert_eq!(decode_secret("sk-plain"), "sk-plain");
        assert_eq!(
            decode_secret(r#"{"version":1,"secret":"sk-wrapped"}"#),
            "sk-wrapped"
        );
    }

    // ─── CachingVault ───

    #[test]
    fn repeated_reads_hit_the_store_once() {
        // The whole point: a dictation reads the STT and LLM keys every time,
        // and on macOS each read can be a fresh authorization prompt.
        let inner = Arc::new(MemoryVault::new());
        inner
            .write(&CredentialId::stt("deepgram"), "dg-key")
            .unwrap();
        let vault = CachingVault::new(inner.clone());
        let id = CredentialId::stt("deepgram");

        for _ in 0..10 {
            assert_eq!(vault.read(&id).unwrap(), Some("dg-key".to_string()));
        }

        assert_eq!(
            inner.read_count(),
            1,
            "only the first read may reach the store"
        );
    }

    #[test]
    fn each_account_is_cached_separately() {
        let inner = Arc::new(MemoryVault::new());
        inner.write(&CredentialId::stt("deepgram"), "dg").unwrap();
        inner.write(&CredentialId::llm("groq"), "gq").unwrap();
        let vault = CachingVault::new(inner.clone());

        for _ in 0..5 {
            vault.read(&CredentialId::stt("deepgram")).unwrap();
            vault.read(&CredentialId::llm("groq")).unwrap();
        }

        assert_eq!(inner.read_count(), 2, "one read per distinct account");
        assert_eq!(
            vault.read(&CredentialId::llm("groq")).unwrap(),
            Some("gq".to_string()),
            "cache must not cross accounts"
        );
    }

    #[test]
    fn a_failed_read_is_never_cached() {
        // A locked keychain must keep reporting itself. Caching the error would
        // turn a transient lock into a session-long outage.
        let inner = Arc::new(MemoryVault::failing("keychain is locked"));
        let vault = CachingVault::new(inner.clone());
        let id = CredentialId::stt("deepgram");

        assert!(vault.read(&id).is_err());
        assert!(vault.read(&id).is_err());
        assert_eq!(inner.read_count(), 2, "every read must retry the store");
    }

    #[test]
    fn a_miss_is_never_cached() {
        // Otherwise a key that appears out of band stays invisible until restart.
        let inner = Arc::new(MemoryVault::new());
        let vault = CachingVault::new(inner.clone());
        let id = CredentialId::stt("deepgram");

        assert_eq!(vault.read(&id).unwrap(), None);
        inner.write(&id, "arrived-later").unwrap();

        assert_eq!(
            vault.read(&id).unwrap(),
            Some("arrived-later".to_string()),
            "a cached miss would hide this"
        );
    }

    #[test]
    fn writing_refreshes_the_cached_value() {
        let inner = Arc::new(MemoryVault::new());
        inner.write(&CredentialId::llm("groq"), "old").unwrap();
        let vault = CachingVault::new(inner.clone());
        let id = CredentialId::llm("groq");

        assert_eq!(vault.read(&id).unwrap(), Some("old".to_string()));
        vault.write(&id, "new").unwrap();

        assert_eq!(
            vault.read(&id).unwrap(),
            Some("new".to_string()),
            "a stale cache would keep authenticating with the replaced key"
        );
        assert_eq!(
            inner.read_count(),
            1,
            "the write should not force a re-read"
        );
    }

    #[test]
    fn a_rejected_write_does_not_poison_the_cache() {
        // If the store refused the value, serving it from cache would report a
        // key as working that is not actually persisted.
        let inner = Arc::new(MemoryVault::failing("keychain is locked"));
        let vault = CachingVault::new(inner.clone());
        let id = CredentialId::llm("groq");

        assert!(vault.write(&id, "never-stored").is_err());
        assert!(
            vault.read(&id).is_err(),
            "must not serve the rejected value"
        );
    }

    #[test]
    fn deleting_drops_the_cached_value() {
        let inner = Arc::new(MemoryVault::new());
        inner.write(&CredentialId::llm("groq"), "gq").unwrap();
        let vault = CachingVault::new(inner.clone());
        let id = CredentialId::llm("groq");

        assert_eq!(vault.read(&id).unwrap(), Some("gq".to_string()));
        vault.delete(&id).unwrap();

        assert_eq!(
            vault.read(&id).unwrap(),
            None,
            "removed key must stay removed"
        );
    }

    // ─── Migration ───

    fn legacy_config() -> serde_json::Value {
        json!({
            "stt_provider": "deepgram",
            "stt_api_key": "dg-legacy",
            "llm_provider": "openrouter",
            "llm_api_key": "or-legacy",
            "polish_enabled": true,
        })
    }

    #[test]
    fn migrates_plaintext_api_keys_and_clears_config_after_success() {
        let vault = MemoryVault::new();
        let mut config = legacy_config();

        let outcome = migrate_legacy_config_secrets(&vault, &mut config);

        assert!(outcome.config_mutated);
        assert_eq!(outcome.migrated, vec!["stt:deepgram", "llm:openrouter"]);
        assert!(outcome.failed.is_empty());

        // Secrets are in the vault…
        assert_eq!(
            vault.read(&CredentialId::stt("deepgram")).unwrap(),
            Some("dg-legacy".to_string())
        );
        assert_eq!(
            vault.read(&CredentialId::llm("openrouter")).unwrap(),
            Some("or-legacy".to_string())
        );
        // …and gone from the config, with everything else preserved.
        assert_eq!(
            config,
            json!({
                "stt_provider": "deepgram",
                "llm_provider": "openrouter",
                "polish_enabled": true,
            })
        );
    }

    #[test]
    fn vault_failure_keeps_the_plaintext() {
        // The case that makes the ordering load-bearing: if the write fails and
        // we had already cleared the field, the user's only copy of the key is
        // gone. They keep it, and the next launch retries.
        let vault = MemoryVault::failing("keychain is locked");
        let mut config = legacy_config();

        let outcome = migrate_legacy_config_secrets(&vault, &mut config);

        assert!(!outcome.config_mutated);
        assert!(outcome.migrated.is_empty());
        assert_eq!(outcome.failed, vec!["stt:deepgram", "llm:openrouter"]);
        assert_eq!(config, legacy_config());
    }

    #[test]
    fn partial_failure_only_clears_what_was_written() {
        // A vault that accepts the STT write and rejects the LLM one is not
        // something MemoryVault can express, so approximate it: pre-seed the
        // LLM slot's provider as empty so only that half fails.
        let vault = MemoryVault::new();
        let mut config = json!({
            "stt_provider": "deepgram",
            "stt_api_key": "dg-legacy",
            "llm_provider": "",
            "llm_api_key": "or-legacy",
        });

        let outcome = migrate_legacy_config_secrets(&vault, &mut config);

        assert!(outcome.config_mutated);
        assert_eq!(outcome.migrated, vec!["stt:deepgram"]);
        assert_eq!(outcome.failed, vec!["llm_api_key"]);
        assert_eq!(
            config,
            json!({
                "stt_provider": "deepgram",
                "llm_provider": "",
                "llm_api_key": "or-legacy",
            }),
            "the un-migrated key must survive"
        );
    }

    #[test]
    fn empty_legacy_keys_are_dropped_without_touching_the_vault() {
        let vault = MemoryVault::failing("must not be called");
        let mut config = json!({
            "stt_provider": "deepgram",
            "stt_api_key": "",
            "llm_provider": "openrouter",
            "llm_api_key": "",
        });

        let outcome = migrate_legacy_config_secrets(&vault, &mut config);

        assert!(outcome.config_mutated);
        assert!(outcome.migrated.is_empty());
        assert!(outcome.failed.is_empty());
        assert_eq!(
            config,
            json!({ "stt_provider": "deepgram", "llm_provider": "openrouter" })
        );
    }

    #[test]
    fn already_migrated_config_is_untouched() {
        let vault = MemoryVault::new();
        let mut config = json!({ "stt_provider": "deepgram", "llm_provider": "openrouter" });
        let snapshot = config.clone();

        let outcome = migrate_legacy_config_secrets(&vault, &mut config);

        assert_eq!(outcome, MigrationOutcome::default());
        assert_eq!(config, snapshot);
    }

    #[test]
    fn migration_is_idempotent() {
        let vault = MemoryVault::new();
        let mut config = legacy_config();
        migrate_legacy_config_secrets(&vault, &mut config);
        let after_first = config.clone();

        let outcome = migrate_legacy_config_secrets(&vault, &mut config);

        assert!(!outcome.config_mutated);
        assert_eq!(config, after_first);
    }

    #[test]
    fn existing_vault_entry_wins_over_stale_plaintext() {
        let vault = MemoryVault::new();
        vault
            .write(&CredentialId::stt("deepgram"), "dg-current")
            .unwrap();
        let mut config = json!({ "stt_provider": "deepgram", "stt_api_key": "dg-stale" });

        let outcome = migrate_legacy_config_secrets(&vault, &mut config);

        assert!(outcome.config_mutated);
        assert!(outcome.migrated.is_empty());
        assert_eq!(
            vault.read(&CredentialId::stt("deepgram")).unwrap(),
            Some("dg-current".to_string()),
            "the vault entry must not be overwritten by stale plaintext"
        );
        assert_eq!(config, json!({ "stt_provider": "deepgram" }));
    }

    #[test]
    fn non_object_value_is_ignored() {
        let vault = MemoryVault::new();
        let mut config = json!("not an object");
        let outcome = migrate_legacy_config_secrets(&vault, &mut config);
        assert_eq!(outcome, MigrationOutcome::default());
    }

    #[test]
    fn non_string_legacy_field_is_dropped_as_empty() {
        let vault = MemoryVault::new();
        let mut config = json!({ "stt_provider": "deepgram", "stt_api_key": 42 });
        let outcome = migrate_legacy_config_secrets(&vault, &mut config);
        assert!(outcome.config_mutated);
        assert_eq!(config, json!({ "stt_provider": "deepgram" }));
    }
}
