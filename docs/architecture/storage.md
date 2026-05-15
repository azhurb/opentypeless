# Storage

OpenTypeless uses local app data for config, history, dictionary, and window/onboarding state. See [Feature map](../domain/features.md) and [Pipeline](pipeline.md) for how stored values feed user-facing behavior.

Evidence: `src-tauri/src/storage/mod.rs`, `src-tauri/migrations/001_init.sql`, `src/lib/tauri.ts`, `src/App.tsx`.

## Config (`tauri-plugin-store`)

- File: `settings.json` in the OS app-data directory.
- Keys in that file: `app_config` (Rust `storage::AppConfig`), `window_state`, `onboarding_completed` (set from the frontend).
- Manager: `ConfigManager` caches the deserialized config in memory and writes updates back to the store.

### `AppConfig` defaults

Verified against `src-tauri/src/storage/mod.rs::Default::default`:

| Field | Default |
| --- | --- |
| `stt_provider` | `glm-asr` |
| `llm_provider` | `openrouter` |
| `llm_model` | `google/gemini-2.5-flash` |
| `polish_enabled` | `true` |
| `translate_enabled` | `false` |
| `target_lang` | `en` |
| `hotkey` | `Alt+/` (macOS) / `Ctrl+/` (other) |
| `hotkey_mode` | `hold` |
| `close_to_tray` | `true` |
| `max_recording_seconds` | `30` |
| `learn_from_corrections_enabled` | `false` |

If you add or change a default, update this table in the same PR.

## SQLite (`<app_data_dir>/opentypeless.db`)

Both stores use the same database file via `rusqlite` (bundled). Tables are created at startup with `CREATE TABLE IF NOT EXISTS` directly inside `HistoryStore::new` and `DictionaryStore::new`. The dictionary table also runs a forward `ALTER TABLE` ladder gated by `PRAGMA user_version` (see below).

### History (`HistoryStore`)

Columns currently created by Rust code:

- `id`, `created_at`, `app_name`, `app_type`, `raw_text`, `polished_text`, `language`, `duration_ms`.

Retention: `MAX_HISTORY_ENTRIES` is 5000. Older rows are pruned on every insert.

### Dictionary (`DictionaryStore`)

Schema version: `1` (tracked via `PRAGMA user_version`).

Columns:

- `id INTEGER PRIMARY KEY AUTOINCREMENT`
- `word TEXT NOT NULL`
- `pronunciation TEXT` (optional, used by manual entries)
- `source TEXT NOT NULL DEFAULT 'manual'` — one of `manual` (added via Settings → Dictionary) or `user_edits` (auto-learned from a correction by the watcher in `src-tauri/src/correction/`).
- `observed_source TEXT` (nullable) — for `user_edits` rows, the STT-produced word the user replaced. Surfaced in the Settings UI tooltip and in the toast copy.
- `frequency_used INTEGER NOT NULL DEFAULT 0` — initialized to `1` for `user_edits` inserts (the edit itself counts as the first use); `0` for manual inserts. Not yet bumped on subsequent dictation use — see the [learn-from-corrections handoff](../superpowers/notes/2026-05-14-learn-from-corrections-handoff.md) for the follow-up plan.
- `last_used TEXT` (nullable) — SQLite `CURRENT_TIMESTAMP` (UTC), set at insert time for `user_edits`, `NULL` for manual.

Insert API has two intents:

- `DictionaryStore::add_manual(word, pronunciation)` — Settings → Dictionary "Add" form.
- `DictionaryStore::add_learned(word, observed_source)` — correction watcher.

Words are loaded before recording and injected into prompt building so custom terms are preserved (see `src-tauri/src/llm/prompt.rs`). `DictionaryStore::words()` returns only the `word` column, ignoring provenance.

Migration ladder: at `DictionaryStore::new`, the runtime ensures the legacy three-column table exists, reads `user_version`, and if `< 1` runs `ALTER TABLE ADD COLUMN` for `source`, `observed_source`, `frequency_used`, `last_used`, then sets `PRAGMA user_version = 1`. Idempotent across repeated opens. Legacy rows migrate in place with `source = 'manual'`.

## `migrations/001_init.sql` is reference-only

`src-tauri/migrations/001_init.sql` declares richer schemas (`stt_provider`, `llm_provider`, `usage_count`, `idx_history_created`, `idx_dictionary_word`). Grep confirms the file is not loaded by any runtime code — the runtime always uses the narrower `CREATE TABLE IF NOT EXISTS` blocks above. Treat the SQL file as a future-schema sketch, not as an executed migration.

If the runtime ever starts executing migrations, this section must be updated.

## Needs confirmation

- Whether the extra columns in `001_init.sql` (`stt_provider`, `llm_provider`, `usage_count`) are planned for a future migration runner, or should be removed from the file to avoid drift.
