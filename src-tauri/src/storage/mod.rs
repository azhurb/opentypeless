use crate::credentials::{migrate_legacy_config_secrets, CredentialVault, LEGACY_SECRET_FIELDS};
use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri_plugin_store::StoreExt;

/// User settings, persisted to `settings.json` as the `app_config` key.
///
/// Deliberately holds **no secrets**. Provider API keys live in the OS
/// credential vault (see [`crate::credentials`]); the `stt_api_key` /
/// `llm_api_key` fields this struct used to carry were plaintext on disk, and
/// `migrate_legacy_config_secrets` moves any left over from an older install.
/// Serde ignores the leftover fields, so an un-migrated config still parses.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub stt_provider: String,
    pub stt_languages: Vec<String>,
    pub llm_provider: String,
    pub llm_model: String,
    pub llm_base_url: String,
    pub polish_enabled: bool,
    pub translate_enabled: bool,
    pub target_lang: String,
    pub hotkey: String,
    pub hotkey_mode: String,
    pub selected_text_enabled: bool,
    pub theme: String,
    pub auto_start: bool,
    pub close_to_tray: bool,
    pub max_recording_seconds: u32,
    pub ui_language: String,
    pub capsule_auto_hide: bool,
    pub learn_from_corrections_enabled: bool,
    /// When false, completed dictations are typed but never written to the
    /// history table. Rows already stored stay readable.
    pub history_enabled: bool,
    /// Age limit for history rows, in days. `0` means keep forever.
    pub history_retention_days: u32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            stt_provider: "glm-asr".to_string(),
            stt_languages: Vec::new(),
            llm_provider: "openrouter".to_string(),
            llm_model: "google/gemini-2.5-flash".to_string(),
            llm_base_url: "https://openrouter.ai/api/v1".to_string(),
            polish_enabled: true,
            translate_enabled: false,
            target_lang: "en".to_string(),
            #[cfg(target_os = "macos")]
            hotkey: "Alt+/".to_string(),
            #[cfg(not(target_os = "macos"))]
            hotkey: "Ctrl+/".to_string(),
            hotkey_mode: "hold".to_string(),
            selected_text_enabled: false,
            theme: "system".to_string(),
            auto_start: false,
            close_to_tray: true,
            max_recording_seconds: 30,
            ui_language: "en".to_string(),
            capsule_auto_hide: false,
            learn_from_corrections_enabled: false,
            history_enabled: true,
            history_retention_days: 0,
        }
    }
}

/// One-shot migration applied at config load time.
///
/// Converts the legacy single-string `stt_language` field into the new
/// multi-value `stt_languages` array. The legacy sentinel `"multi"` and any
/// empty string become an empty array (auto-detect); any other code becomes
/// a singleton list.
///
/// Returns `true` if `value` was mutated.
fn migrate_legacy_config(value: &mut serde_json::Value) -> bool {
    let Some(obj) = value.as_object_mut() else {
        return false;
    };
    if !obj.contains_key("stt_language") {
        return false;
    }
    let legacy = obj.remove("stt_language");
    if !obj.contains_key("stt_languages") {
        let codes: Vec<String> = match legacy {
            Some(serde_json::Value::String(s)) if !s.is_empty() && s != "multi" => vec![s],
            _ => Vec::new(),
        };
        obj.insert("stt_languages".to_string(), serde_json::json!(codes));
    }
    true
}

// ─── ConfigManager (tauri-plugin-store backed) ───

pub struct ConfigManager {
    app_handle: tauri::AppHandle,
    cache: Mutex<Option<AppConfig>>,
    /// Held so `load` can run the plaintext-secret migration against the same
    /// vault the rest of the app reads from.
    vault: Arc<dyn CredentialVault>,
    /// Legacy plaintext secrets the vault refused to accept, re-attached by
    /// every `save`.
    ///
    /// `AppConfig` no longer models these fields, so serializing it drops them.
    /// Without this, a launch where the vault is locked followed by any Settings
    /// save (changing the theme is enough) would erase the user's only copy of a
    /// key that never made it into the vault. The next launch retries the
    /// migration and this empties out.
    pending_legacy_secrets: Mutex<serde_json::Map<String, serde_json::Value>>,
}

impl ConfigManager {
    pub fn new(app_handle: tauri::AppHandle, vault: Arc<dyn CredentialVault>) -> Self {
        Self {
            app_handle,
            cache: Mutex::new(None),
            vault,
            pending_legacy_secrets: Mutex::new(serde_json::Map::new()),
        }
    }

    pub async fn load(&self) -> Result<AppConfig> {
        if let Some(config) = self.cache.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            return Ok(config);
        }

        let (config, migrated) = match self.app_handle.store("settings.json") {
            Ok(store) => match store.get("app_config") {
                Some(val) => {
                    let mut v = val.clone();
                    let mut mutated = migrate_legacy_config(&mut v);

                    // Move any plaintext API keys into the credential vault.
                    // Anything the vault would not take stays in `v` and is
                    // remembered so `save` re-attaches it.
                    let secrets = migrate_legacy_config_secrets(self.vault.as_ref(), &mut v);
                    mutated |= secrets.config_mutated;
                    if !secrets.migrated.is_empty() {
                        tracing::info!(
                            "moved {} plaintext API key(s) into the credential vault: {}",
                            secrets.migrated.len(),
                            secrets.migrated.join(", ")
                        );
                    }
                    if !secrets.failed.is_empty() {
                        tracing::warn!(
                            "could not vault {} API key(s), leaving them in settings.json \
                             and retrying next launch: {}",
                            secrets.failed.len(),
                            secrets.failed.join(", ")
                        );
                    }
                    self.remember_pending_legacy_secrets(&v);

                    let parsed = match serde_json::from_value::<AppConfig>(v) {
                        Ok(config) => config,
                        Err(e) => {
                            // We still have to start, but falling back silently is
                            // not acceptable: the defaults re-enable opt-outs the
                            // user deliberately turned off (history recording,
                            // learn-from-corrections), and if the value also needs
                            // legacy migration we then persist those defaults over
                            // their settings. At minimum the reason must be logged.
                            tracing::error!(
                                "failed to parse app_config, falling back to defaults \
                                 (this re-enables history recording and other opt-outs): {}",
                                e
                            );
                            AppConfig::default()
                        }
                    };
                    (parsed, mutated)
                }
                None => (AppConfig::default(), false),
            },
            Err(e) => {
                tracing::warn!("failed to open settings.json store, using defaults: {}", e);
                (AppConfig::default(), false)
            }
        };

        *self.cache.lock().unwrap_or_else(|e| e.into_inner()) = Some(config.clone());

        if migrated {
            // Best-effort: persist the migrated shape so future loads are clean.
            // If the write fails (locked file, etc.) we still have the in-memory
            // migrated config — next save will overwrite anyway.
            let _ = self.save(&config).await;
        }

        Ok(config)
    }

    /// Snapshot the legacy secret fields still present after the migration ran,
    /// so `save` can put them back. Empty on the happy path.
    fn remember_pending_legacy_secrets(&self, value: &serde_json::Value) {
        let mut pending = serde_json::Map::new();
        if let Some(obj) = value.as_object() {
            for field in LEGACY_SECRET_FIELDS {
                if let Some(v) = obj.get(field) {
                    pending.insert(field.to_string(), v.clone());
                }
            }
        }
        *self
            .pending_legacy_secrets
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = pending;
    }

    pub async fn save(&self, config: &AppConfig) -> Result<()> {
        *self.cache.lock().unwrap_or_else(|e| e.into_inner()) = Some(config.clone());

        let store = self
            .app_handle
            .store("settings.json")
            .map_err(|e| anyhow::anyhow!("Failed to open store: {}", e))?;
        let mut val = serde_json::to_value(config)?;
        // Carry forward any plaintext key the vault rejected — see
        // `pending_legacy_secrets`.
        {
            let pending = self
                .pending_legacy_secrets
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let (Some(obj), false) = (val.as_object_mut(), pending.is_empty()) {
                for (field, secret) in pending.iter() {
                    obj.insert(field.clone(), secret.clone());
                }
            }
        }
        store.set("app_config", val);
        store.save().map_err(|e| anyhow::anyhow!("{}", e))?;

        Ok(())
    }
}

// ─── HistoryStore (SQLite backed) ───

/// Hard backstop on history size, applied regardless of the user's age-based
/// retention setting. Older entries are pruned on insert.
const MAX_HISTORY_ENTRIES: u32 = 5000;

/// Format `created_at` is written in — naive **local** time, fixed width, so
/// lexicographic string ordering equals chronological ordering. Age-based
/// pruning builds its cutoff with this same format and compares as text; using
/// UTC here would skew every cutoff by the machine's offset.
pub const HISTORY_TIMESTAMP_FORMAT: &str = "%Y-%m-%dT%H:%M:%S";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: i64,
    pub created_at: String,
    pub app_name: String,
    pub app_type: String,
    pub raw_text: String,
    pub polished_text: String,
    pub language: Option<String>,
    pub duration_ms: Option<i64>,
}

/// Fold the WAL back into the main database and truncate it. `secure_delete`
/// scrubs pages in the main file, but the deleted rows' text can still sit in the
/// `-wal` sidecar until a checkpoint happens on its own schedule. Best-effort: a
/// failed checkpoint is not worth failing a delete over, and the next one retries.
///
/// Note this scrubs content, it does not shrink the file — reclaiming space would
/// need a `VACUUM`, which is not worth blocking on here.
fn checkpoint_wal(conn: &Connection) {
    if let Err(e) = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);") {
        tracing::warn!("history wal checkpoint after delete failed: {}", e);
    }
}

/// Upper bound on `history_retention_days`, ~100 years. `history_retention_days`
/// is a `u32` read from `settings.json`, so a hand edit, a corrupted write, or a
/// bad import can hand us a value that overflows chrono — and `chrono`'s `Sub`
/// **panics** rather than erroring. The startup prune runs inside Tauri `setup`,
/// where that panic aborts launch with no in-app way to recover, so the value is
/// clamped before it reaches chrono at all.
const MAX_RETENTION_DAYS: u32 = 36_500;

/// The `created_at` string every row older than the retention window sorts
/// before. `None` when retention is "forever" (`0`) or when the subtraction
/// would leave the representable date range — both mean "prune nothing", which
/// is the safe direction to fail in.
fn retention_cutoff(retention_days: u32) -> Option<String> {
    if retention_days == 0 {
        return None;
    }
    let days = i64::from(retention_days.min(MAX_RETENTION_DAYS));
    let cutoff = chrono::Local::now().checked_sub_signed(chrono::Duration::days(days))?;
    Some(cutoff.format(HISTORY_TIMESTAMP_FORMAT).to_string())
}

pub struct HistoryStore {
    conn: Mutex<Connection>,
}

impl HistoryStore {
    pub fn new(db_path: PathBuf) -> Result<Self> {
        let conn = Connection::open(&db_path)?;
        // `secure_delete` overwrites freed pages instead of just returning them to
        // the freelist. Without it, retention and "Clear all" leave the transcript
        // text of "deleted" dictations readable in the db file — the exact thing an
        // age limit on a privacy-first app is supposed to prevent. The write cost is
        // irrelevant for a table this small.
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA secure_delete=ON;")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at TEXT NOT NULL,
                app_name TEXT NOT NULL DEFAULT '',
                app_type TEXT NOT NULL DEFAULT '',
                raw_text TEXT NOT NULL DEFAULT '',
                polished_text TEXT NOT NULL DEFAULT '',
                language TEXT,
                duration_ms INTEGER
            );",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Insert one entry, then apply both retention rules under the same lock:
    /// the `MAX_HISTORY_ENTRIES` backstop and, when `retention_days > 0`, the
    /// user's age limit. Keeping policy here means no caller can insert past it.
    pub async fn add(&self, entry: HistoryEntry, retention_days: u32) -> Result<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "INSERT INTO history (created_at, app_name, app_type, raw_text, polished_text, language, duration_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                entry.created_at,
                entry.app_name,
                entry.app_type,
                entry.raw_text,
                entry.polished_text,
                entry.language,
                entry.duration_ms,
            ],
        )?;

        // Prune old entries beyond the retention limit
        conn.execute(
            "DELETE FROM history WHERE id NOT IN (SELECT id FROM history ORDER BY id DESC LIMIT ?1)",
            rusqlite::params![MAX_HISTORY_ENTRIES],
        )?;

        if let Some(cutoff) = retention_cutoff(retention_days) {
            conn.execute(
                "DELETE FROM history WHERE created_at < ?1",
                rusqlite::params![cutoff],
            )?;
        }

        Ok(())
    }

    /// Delete every row older than `retention_days`. Returns the number of rows
    /// removed; `retention_days == 0` means "keep forever" and is a no-op.
    ///
    /// Called at startup and after a config save, not just on insert — with
    /// history disabled nothing is ever inserted, so insert-time pruning alone
    /// would let rows outlive their retention window indefinitely.
    pub async fn prune_older_than(&self, retention_days: u32) -> Result<usize> {
        let Some(cutoff) = retention_cutoff(retention_days) else {
            return Ok(0);
        };
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let removed = conn.execute(
            "DELETE FROM history WHERE created_at < ?1",
            rusqlite::params![cutoff],
        )?;
        if removed > 0 {
            checkpoint_wal(&conn);
        }
        Ok(removed)
    }

    pub async fn list(&self, limit: u32, offset: u32) -> Result<Vec<HistoryEntry>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn.prepare(
            "SELECT id, created_at, app_name, app_type, raw_text, polished_text, language, duration_ms
             FROM history ORDER BY id DESC LIMIT ?1 OFFSET ?2"
        )?;
        let rows = stmt.query_map(rusqlite::params![limit, offset], |row| {
            Ok(HistoryEntry {
                id: row.get(0)?,
                created_at: row.get(1)?,
                app_name: row.get(2)?,
                app_type: row.get(3)?,
                raw_text: row.get(4)?,
                polished_text: row.get(5)?,
                language: row.get(6)?,
                duration_ms: row.get(7)?,
            })
        })?;
        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }
        Ok(entries)
    }

    pub async fn clear(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute("DELETE FROM history", [])?;
        checkpoint_wal(&conn);
        Ok(())
    }
}

// ─── DictionaryStore (SQLite backed) ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictionaryEntry {
    pub id: i64,
    pub word: String,
    pub pronunciation: Option<String>,
    /// One of "manual", "user_edits". String not enum to keep SQLite-side flexible.
    pub source: String,
    /// For source = "user_edits": the STT-produced word the user replaced.
    pub observed_source: Option<String>,
    pub frequency_used: i64,
    /// SQLite `CURRENT_TIMESTAMP` format: "YYYY-MM-DD HH:MM:SS" (UTC). Optional.
    pub last_used: Option<String>,
}

pub struct DictionaryStore {
    conn: Mutex<Connection>,
}

impl DictionaryStore {
    pub fn new(db_path: PathBuf) -> Result<Self> {
        let conn = Connection::open(&db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;

        // Legacy base table — never edit this; migrations evolve it forward.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS dictionary (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                word TEXT NOT NULL,
                pronunciation TEXT
            );",
        )?;

        let version: i32 =
            conn.query_row("SELECT user_version FROM pragma_user_version", [], |r| {
                r.get(0)
            })?;

        if version < 1 {
            conn.execute_batch(
                "ALTER TABLE dictionary ADD COLUMN source TEXT NOT NULL DEFAULT 'manual';
                 ALTER TABLE dictionary ADD COLUMN observed_source TEXT;
                 ALTER TABLE dictionary ADD COLUMN frequency_used INTEGER NOT NULL DEFAULT 0;
                 ALTER TABLE dictionary ADD COLUMN last_used TEXT;
                 PRAGMA user_version = 1;",
            )?;
        }

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Insert a manually-added entry (Settings → Dictionary "Add" button).
    /// frequency_used=0, last_used=NULL, source='manual', observed_source=NULL.
    pub async fn add_manual(&self, word: &str, pronunciation: Option<&str>) -> Result<i64> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "INSERT INTO dictionary (word, pronunciation, source, observed_source, frequency_used, last_used)
             VALUES (?1, ?2, 'manual', NULL, 0, NULL)",
            rusqlite::params![word, pronunciation],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Insert an auto-learned entry from a single-word correction.
    /// `observed_source` is the STT-produced word the user replaced.
    /// frequency_used=1 (this edit counts as the first use), last_used=CURRENT_TIMESTAMP.
    pub async fn add_learned(&self, word: &str, observed_source: &str) -> Result<i64> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "INSERT INTO dictionary (word, pronunciation, source, observed_source, frequency_used, last_used)
             VALUES (?1, NULL, 'user_edits', ?2, 1, CURRENT_TIMESTAMP)",
            rusqlite::params![word, observed_source],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub async fn remove(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "DELETE FROM dictionary WHERE id = ?1",
            rusqlite::params![id],
        )?;
        Ok(())
    }

    pub async fn list(&self) -> Result<Vec<DictionaryEntry>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn.prepare(
            "SELECT id, word, pronunciation, source, observed_source, frequency_used, last_used
             FROM dictionary",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(DictionaryEntry {
                id: row.get(0)?,
                word: row.get(1)?,
                pronunciation: row.get(2)?,
                source: row.get(3)?,
                observed_source: row.get(4)?,
                frequency_used: row.get(5)?,
                last_used: row.get(6)?,
            })
        })?;
        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }
        Ok(entries)
    }

    pub async fn words(&self) -> Vec<String> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = match conn.prepare("SELECT word FROM dictionary") {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = match stmt.query_map([], |row| row.get::<_, String>(0)) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };
        rows.filter_map(|r| r.ok()).collect()
    }
}

#[cfg(test)]
mod config_migration_tests {
    use super::migrate_legacy_config;
    use serde_json::json;

    #[test]
    fn legacy_multi_becomes_empty_set() {
        let mut v = json!({ "stt_language": "multi" });
        let mutated = migrate_legacy_config(&mut v);
        assert!(mutated);
        assert_eq!(v, json!({ "stt_languages": [] }));
    }

    #[test]
    fn legacy_single_code_becomes_singleton_set() {
        let mut v = json!({ "stt_language": "en" });
        let mutated = migrate_legacy_config(&mut v);
        assert!(mutated);
        assert_eq!(v, json!({ "stt_languages": ["en"] }));
    }

    #[test]
    fn legacy_empty_string_becomes_empty_set() {
        let mut v = json!({ "stt_language": "" });
        let mutated = migrate_legacy_config(&mut v);
        assert!(mutated);
        assert_eq!(v, json!({ "stt_languages": [] }));
    }

    #[test]
    fn already_migrated_config_is_untouched() {
        let mut v = json!({ "stt_languages": ["en", "de"] });
        let snapshot = v.clone();
        let mutated = migrate_legacy_config(&mut v);
        assert!(!mutated);
        assert_eq!(v, snapshot);
    }

    #[test]
    fn both_keys_present_prefers_stt_languages_and_drops_legacy() {
        let mut v = json!({ "stt_language": "de", "stt_languages": ["en"] });
        let mutated = migrate_legacy_config(&mut v);
        assert!(mutated);
        assert_eq!(v, json!({ "stt_languages": ["en"] }));
    }

    #[test]
    fn config_without_either_key_is_untouched() {
        let mut v = json!({ "stt_provider": "glm-asr" });
        let snapshot = v.clone();
        let mutated = migrate_legacy_config(&mut v);
        assert!(!mutated);
        assert_eq!(v, snapshot);
    }

    #[test]
    fn non_object_value_returns_false_without_panic() {
        let mut v = json!("not an object");
        let mutated = migrate_legacy_config(&mut v);
        assert!(!mutated);
        assert_eq!(v, json!("not an object"));
    }

    #[test]
    fn other_fields_are_preserved_on_migration() {
        let mut v = json!({
            "stt_provider": "glm-asr",
            "stt_language": "de",
            "polish_enabled": true,
        });
        let mutated = migrate_legacy_config(&mut v);
        assert!(mutated);
        assert_eq!(
            v,
            json!({
                "stt_provider": "glm-asr",
                "stt_languages": ["de"],
                "polish_enabled": true,
            })
        );
    }
}

#[cfg(test)]
mod dictionary_tests {
    use super::*;

    fn uuid_like() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("{}-{}", nanos, N.fetch_add(1, Ordering::Relaxed))
    }

    fn temp_store() -> DictionaryStore {
        let dir = std::env::temp_dir().join(format!("otl-dict-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("dict-{}.db", uuid_like()));
        DictionaryStore::new(path).unwrap()
    }

    #[tokio::test]
    async fn add_manual_inserts_with_manual_source() {
        let store = temp_store();
        let id = store.add_manual("Tim", Some("tihm")).await.unwrap();
        let listed = store.list().await.unwrap();
        let entry = listed.iter().find(|e| e.id == id).unwrap();
        assert_eq!(entry.word, "Tim");
        assert_eq!(entry.pronunciation.as_deref(), Some("tihm"));
        assert_eq!(entry.source, "manual");
        assert!(entry.observed_source.is_none());
        assert_eq!(entry.frequency_used, 0);
        assert!(entry.last_used.is_none());
    }

    #[tokio::test]
    async fn add_learned_inserts_with_observed_source_and_initial_use() {
        let store = temp_store();
        let id = store.add_learned("Vlad", "Vladislav").await.unwrap();
        let listed = store.list().await.unwrap();
        let entry = listed.iter().find(|e| e.id == id).unwrap();
        assert_eq!(entry.word, "Vlad");
        assert!(entry.pronunciation.is_none());
        assert_eq!(entry.source, "user_edits");
        assert_eq!(entry.observed_source.as_deref(), Some("Vladislav"));
        assert_eq!(entry.frequency_used, 1);
        assert!(
            entry.last_used.is_some(),
            "add_learned must stamp last_used"
        );
    }

    #[tokio::test]
    async fn add_methods_return_monotonic_row_ids() {
        let store = temp_store();
        let id1 = store.add_manual("Timmy", None).await.unwrap();
        let id2 = store.add_learned("Tim", "Timmy").await.unwrap();
        assert!(id2 > id1, "row ids must monotonically increase");
        let listed = store.list().await.unwrap();
        assert_eq!(listed.len(), 2);
    }

    #[tokio::test]
    async fn migrates_legacy_v0_database_to_v1() {
        let dir = std::env::temp_dir().join(format!("otl-dict-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("legacy-{}.db", uuid_like()));

        // Simulate a v0 database (the schema this code shipped with originally).
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE dictionary (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    word TEXT NOT NULL,
                    pronunciation TEXT
                );
                INSERT INTO dictionary (word, pronunciation) VALUES ('Vlad', NULL);",
            )
            .unwrap();
            // pragma_user_version defaults to 0; no PRAGMA write needed.
        }

        // Re-open via DictionaryStore — migration should fire.
        let store = DictionaryStore::new(path.clone()).unwrap();
        let entries = store.list().await.unwrap();
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e.word, "Vlad");
        assert_eq!(
            e.source, "manual",
            "legacy rows must default to source=manual"
        );
        assert!(e.observed_source.is_none());
        assert_eq!(e.frequency_used, 0);
        assert!(e.last_used.is_none());

        // Verify user_version is now 1.
        let conn = rusqlite::Connection::open(&path).unwrap();
        let v: i32 = conn
            .query_row("SELECT user_version FROM pragma_user_version", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(v, 1);
    }

    #[tokio::test]
    async fn fresh_install_is_at_version_1_with_full_schema() {
        let dir = std::env::temp_dir().join(format!("otl-dict-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("fresh-{}.db", uuid_like()));
        let _store = DictionaryStore::new(path.clone()).unwrap();

        let conn = rusqlite::Connection::open(&path).unwrap();
        let v: i32 = conn
            .query_row("SELECT user_version FROM pragma_user_version", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(v, 1, "fresh install must end at version 1");

        let cols: Vec<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(dictionary)").unwrap();
            let rows = stmt.query_map([], |r| r.get::<_, String>(1)).unwrap();
            rows.filter_map(|r| r.ok()).collect()
        };
        for expected in [
            "id",
            "word",
            "pronunciation",
            "source",
            "observed_source",
            "frequency_used",
            "last_used",
        ] {
            assert!(
                cols.iter().any(|c| c == expected),
                "missing column: {}",
                expected
            );
        }
    }

    #[tokio::test]
    async fn migration_is_idempotent() {
        let dir = std::env::temp_dir().join(format!("otl-dict-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("idem-{}.db", uuid_like()));

        let _ = DictionaryStore::new(path.clone()).unwrap();
        // Second open must not error (would otherwise re-run ALTERs and fail).
        let _ = DictionaryStore::new(path.clone()).unwrap();
        let _ = DictionaryStore::new(path).unwrap();
    }
}

#[cfg(test)]
mod history_tests {
    use super::*;

    fn uuid_like() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("{}-{}", nanos, N.fetch_add(1, Ordering::Relaxed))
    }

    fn temp_store() -> HistoryStore {
        let dir = std::env::temp_dir().join(format!("otl-hist-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("hist-{}.db", uuid_like()));
        HistoryStore::new(path).unwrap()
    }

    /// A row stamped `days_ago` days before now, in the same local-time format
    /// the pipeline writes.
    fn entry_aged(days_ago: i64, text: &str) -> HistoryEntry {
        let created = chrono::Local::now() - chrono::Duration::days(days_ago);
        HistoryEntry {
            id: 0,
            created_at: created.format(HISTORY_TIMESTAMP_FORMAT).to_string(),
            app_name: "Slack".to_string(),
            app_type: "Chat".to_string(),
            raw_text: text.to_string(),
            polished_text: text.to_string(),
            language: None,
            duration_ms: Some(1200),
        }
    }

    async fn stored_texts(store: &HistoryStore) -> Vec<String> {
        store
            .list(100, 0)
            .await
            .unwrap()
            .into_iter()
            .map(|e| e.polished_text)
            .collect()
    }

    #[tokio::test]
    async fn retention_cutoff_is_none_for_forever() {
        assert!(retention_cutoff(0).is_none());
        assert!(retention_cutoff(30).is_some());
    }

    /// A hand-edited or corrupted `history_retention_days` must not reach chrono
    /// unclamped: chrono *panics* on out-of-range durations, and the startup prune
    /// runs inside Tauri `setup`, where that panic aborts launch for good.
    #[tokio::test]
    async fn retention_cutoff_survives_absurd_day_counts() {
        for days in [u32::MAX, 999_999_999, MAX_RETENTION_DAYS + 1] {
            let cutoff = retention_cutoff(days);
            assert!(
                cutoff.is_some(),
                "clamped cutoff must still be produced for {} days",
                days
            );
            assert_eq!(
                cutoff,
                retention_cutoff(MAX_RETENTION_DAYS),
                "{} days must clamp to MAX_RETENTION_DAYS",
                days
            );
        }
    }

    #[tokio::test]
    async fn absurd_retention_prunes_nothing_instead_of_panicking() {
        let store = temp_store();
        store.add(entry_aged(400, "ancient"), 0).await.unwrap();
        // Clamped to ~100 years, so a 400-day-old row is well inside the window.
        assert_eq!(store.prune_older_than(u32::MAX).await.unwrap(), 0);
        assert_eq!(stored_texts(&store).await.len(), 1);
    }

    #[tokio::test]
    async fn add_survives_absurd_retention() {
        let store = temp_store();
        store.add(entry_aged(0, "fresh"), u32::MAX).await.unwrap();
        assert_eq!(stored_texts(&store).await, vec!["fresh".to_string()]);
    }

    #[tokio::test]
    async fn prune_older_than_zero_keeps_everything() {
        let store = temp_store();
        store.add(entry_aged(400, "ancient"), 0).await.unwrap();
        store.add(entry_aged(0, "fresh"), 0).await.unwrap();

        assert_eq!(store.prune_older_than(0).await.unwrap(), 0);
        assert_eq!(stored_texts(&store).await.len(), 2);
    }

    #[tokio::test]
    async fn prune_older_than_drops_only_rows_past_the_cutoff() {
        let store = temp_store();
        store.add(entry_aged(100, "ancient"), 0).await.unwrap();
        store.add(entry_aged(31, "just-too-old"), 0).await.unwrap();
        store.add(entry_aged(29, "just-inside"), 0).await.unwrap();
        store.add(entry_aged(0, "fresh"), 0).await.unwrap();

        assert_eq!(store.prune_older_than(30).await.unwrap(), 2);
        let remaining = stored_texts(&store).await;
        assert_eq!(remaining.len(), 2);
        assert!(remaining.contains(&"just-inside".to_string()));
        assert!(remaining.contains(&"fresh".to_string()));
    }

    #[tokio::test]
    async fn add_applies_retention_to_already_stored_rows() {
        let store = temp_store();
        // Stored while retention was "forever"…
        store.add(entry_aged(90, "ancient"), 0).await.unwrap();
        // …then the user picks 30 days and dictates again.
        store.add(entry_aged(0, "fresh"), 30).await.unwrap();

        assert_eq!(stored_texts(&store).await, vec!["fresh".to_string()]);
    }

    #[tokio::test]
    async fn clear_removes_every_row() {
        let store = temp_store();
        store.add(entry_aged(1, "one"), 0).await.unwrap();
        store.add(entry_aged(0, "two"), 0).await.unwrap();

        store.clear().await.unwrap();
        assert!(stored_texts(&store).await.is_empty());
    }
}
