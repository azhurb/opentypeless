use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri_plugin_store::StoreExt;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub stt_provider: String,
    pub stt_api_key: String,
    pub stt_language: String,
    pub llm_provider: String,
    pub llm_api_key: String,
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
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            stt_provider: "glm-asr".to_string(),
            stt_api_key: String::new(),
            stt_language: "multi".to_string(),
            llm_provider: "openrouter".to_string(),
            llm_api_key: String::new(),
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
        }
    }
}

// ─── ConfigManager (tauri-plugin-store backed) ───

pub struct ConfigManager {
    app_handle: tauri::AppHandle,
    cache: Mutex<Option<AppConfig>>,
}

impl ConfigManager {
    pub fn new(app_handle: tauri::AppHandle) -> Self {
        Self {
            app_handle,
            cache: Mutex::new(None),
        }
    }

    pub async fn load(&self) -> Result<AppConfig> {
        if let Some(config) = self.cache.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            return Ok(config);
        }

        let config = match self.app_handle.store("settings.json") {
            Ok(store) => match store.get("app_config") {
                Some(val) => serde_json::from_value::<AppConfig>(val.clone()).unwrap_or_default(),
                None => AppConfig::default(),
            },
            Err(_) => AppConfig::default(),
        };

        *self.cache.lock().unwrap_or_else(|e| e.into_inner()) = Some(config.clone());
        Ok(config)
    }

    pub async fn save(&self, config: &AppConfig) -> Result<()> {
        *self.cache.lock().unwrap_or_else(|e| e.into_inner()) = Some(config.clone());

        let store = self
            .app_handle
            .store("settings.json")
            .map_err(|e| anyhow::anyhow!("Failed to open store: {}", e))?;
        let val = serde_json::to_value(config)?;
        store.set("app_config", val);
        store.save().map_err(|e| anyhow::anyhow!("{}", e))?;

        Ok(())
    }
}

// ─── HistoryStore (SQLite backed) ───

/// Maximum number of history entries to retain. Older entries are pruned on insert.
const MAX_HISTORY_ENTRIES: u32 = 5000;

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

pub struct HistoryStore {
    conn: Mutex<Connection>,
}

impl HistoryStore {
    pub fn new(db_path: PathBuf) -> Result<Self> {
        let conn = Connection::open(&db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
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

    pub async fn add(&self, entry: HistoryEntry) -> Result<()> {
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

        Ok(())
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

        let version: i32 = conn.query_row(
            "SELECT user_version FROM pragma_user_version",
            [],
            |r| r.get(0),
        )?;

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
        assert!(entry.last_used.is_some(), "add_learned must stamp last_used");
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
        assert_eq!(e.source, "manual", "legacy rows must default to source=manual");
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
