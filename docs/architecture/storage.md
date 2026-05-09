# Storage

OpenTypeless uses local app data for config, history, dictionary, and window/onboarding state.

Evidence: `src-tauri/src/storage/mod.rs`, `src-tauri/migrations/001_init.sql`, `src/lib/tauri.ts`, `src/App.tsx`.

## Config

Config uses `tauri-plugin-store`.

- File: `settings.json` in the OS app-data directory.
- Key: `app_config`.
- Rust type: `storage::AppConfig`.
- Manager: `ConfigManager`.

`ConfigManager` caches deserialized config in memory and writes updated config back to the store.

Window state is also stored in `settings.json` under `window_state`.

Onboarding completion is stored from the frontend under `onboarding_completed`.

## Config Defaults

Important defaults visible in `storage::AppConfig` and `src/stores/appStore.ts`:

- STT provider: `glm-asr`.
- LLM provider: `openrouter`.
- LLM model: `google/gemini-2.5-flash`.
- Hotkey: `Alt+/` on macOS, `Ctrl+/` elsewhere.
- Hotkey mode: `hold`.
- Output mode: `keyboard`.
- Polish enabled: `true`.
- Translation enabled: `false`.
- Close to tray: `true`.
- Max recording seconds: `30`.

## History

History uses SQLite through `rusqlite`.

- Database path: `<app_data_dir>/opentypeless.db`.
- Rust store: `HistoryStore`.
- Retention: `MAX_HISTORY_ENTRIES` is 5000; older entries are pruned on insert.

Fields used by current Rust structs:

- `id`
- `created_at`
- `app_name`
- `app_type`
- `raw_text`
- `polished_text`
- `language`
- `duration_ms`

## Dictionary

Dictionary uses SQLite through `rusqlite`.

- Database path: same `opentypeless.db`.
- Rust store: `DictionaryStore`.
- Entries include `id`, `word`, and optional `pronunciation`.

Dictionary words are loaded before recording and passed into prompt building so custom terms can be preserved.

## Schema Note

`src-tauri/migrations/001_init.sql` includes extra fields such as `stt_provider`, `llm_provider`, `created_at`, and `usage_count`. The stores also create tables directly with `CREATE TABLE IF NOT EXISTS`.

Needs confirmation: whether migrations are currently executed by runtime code or only retained as schema reference. Initial docs should not assume migration execution until confirmed.
