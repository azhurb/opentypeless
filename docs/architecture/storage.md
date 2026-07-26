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
| `stt_languages` | `[]` (empty = auto-detect) |
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
| `history_enabled` | `true` |
| `history_retention_days` | `0` (= keep forever) |

If you add or change a default, update this table in the same PR.

### Config migrations

`ConfigManager::load` runs `migrate_legacy_config` on the raw JSON value before deserializing into `AppConfig`. The migration is idempotent and so far handles one case:

- Pre-multi-language installs persisted `stt_language: String` (with the sentinel `"multi"`). Load-time migration converts `"multi"` / `""` to `stt_languages = []` and any other code to `[code]`, then removes the legacy key. The migrated config is written back on the same load.

Add new migrations to `migrate_legacy_config` rather than re-mapping fields downstream; tests live in `storage::config_migration_tests`.

### Load failures fail open, loudly

If `app_config` cannot be deserialized, `ConfigManager::load` falls back to
`AppConfig::default()` — the app has to start. That fallback re-enables anything the user
opted out of, including `history_enabled`, and if the value also needed legacy migration the
defaults are then written back over their settings. It therefore logs at `error` with the
serde message. If you add another privacy-relevant flag, this is the fail-open path to think
about.

## SQLite (`<app_data_dir>/opentypeless.db`)

Both stores use the same database file via `rusqlite` (bundled). Tables are created at startup with `CREATE TABLE IF NOT EXISTS` directly inside `HistoryStore::new` and `DictionaryStore::new`. The dictionary table also runs a forward `ALTER TABLE` ladder gated by `PRAGMA user_version` (see below).

### History (`HistoryStore`)

Columns currently created by Rust code:

- `id`, `created_at`, `app_name`, `app_type`, `raw_text`, `polished_text`, `language`, `duration_ms`.

#### Writes are opt-out

The pipeline writes a row only when `history_enabled` is true (`src-tauri/src/pipeline.rs`).
With it false, dictations are still transcribed, polished, and typed — nothing is recorded.
Rows already stored stay readable and searchable, **but retention still applies to them** —
turning saving off is not a way to freeze the archive. Because `HomePage`'s counters are
derived from the history list, they stop advancing while history is off.

The flag is re-read at write time rather than taken from the recording-start config snapshot
(`preloaded_config`), so a user who opts out mid-dictation — possible in `toggle` hotkey mode
— is honored for that dictation. The read is cache-backed and effectively free.

The UI must consult the **persisted** config for this, not the Zustand `config`, which
carries unsaved Settings edits: `src/components/History/index.tsx` reads
`savedConfig ?? config`, so the "saving is off" notice can never claim the backend has
stopped recording before the change is actually saved.

#### Retention

Two rules:

1. **Count backstop** — `MAX_HISTORY_ENTRIES` is 5000, read from a constant inside
   `HistoryStore::add`, so it cannot be bypassed by a caller.
2. **Age limit** — `history_retention_days` (`0` = forever). Settings → General offers
   Forever / 7 / 30 / 90. This one is *caller-supplied* as `add(entry, retention_days)`; a
   caller passing `0` skips it, which is what the tests do deliberately. Any new history
   writer must pass the real config value.

`HistoryStore::prune_older_than(days)` performs the age `DELETE` and is a no-op at `0`. It
runs at four points:

| Site | Why |
| --- | --- |
| `HistoryStore::add` | Trims during a long-running session. |
| After a dictation with saving **off** (`pipeline.rs`) | There is no insert, so `add`'s prune never fires; without this a session left running for weeks would honor the window only at launch. |
| App startup (`lib.rs` `setup`) | Catches a machine that was off past the window. Logs its row count even on success — it is the one destructive prune that runs unattended, so "my history is gone" has to be distinguishable from corruption. |
| `update_config` | A lowered retention applies on Save, not at next launch. Emits `history:changed` when it deleted anything, because `config:changed` only replaces each webview's config copy and the History pane would otherwise keep listing deleted rows. |

Prune failures are logged, never propagated — in `update_config` the config is already
persisted, so failing the save over a `DELETE` would be worse than a stale row.

Narrowing the window is confirmed in the UI before it is applied
(`settings.retentionConfirm`), matching the confirm already required by "Clear All History"
for the same data. Widening, or switching to Forever, deletes nothing and is not confirmed.

**`history_retention_days` is clamped** to `MAX_RETENTION_DAYS` (~100 years) before it
reaches chrono, and the subtraction uses `checked_sub_signed`. chrono *panics* on
out-of-range durations, and the startup prune runs inside Tauri `setup` where a panic aborts
launch with no in-app recovery — so a hand-edited or corrupted `settings.json` must not be
able to reach it. Overflow yields "prune nothing", the safe direction.

**Deleted rows are scrubbed, not just unlinked.** `HistoryStore::new` sets
`PRAGMA secure_delete=ON` so freed pages are overwritten instead of returned to the freelist
readable, and `prune_older_than` / `clear` run `PRAGMA wal_checkpoint(TRUNCATE)` when they
removed anything so the text does not linger in the `-wal` sidecar. Note this scrubs content
but does not shrink the file — that would need a `VACUUM`, which is not worth blocking a
delete on.

**Timestamp invariant.** `created_at` is naive **local** time in the fixed-width format
`storage::HISTORY_TIMESTAMP_FORMAT` (`%Y-%m-%dT%H:%M:%S`), shared by the pipeline's insert
and the prune cutoff. Fixed width means lexicographic ordering equals chronological
ordering, so pruning is a plain `WHERE created_at < ?` string comparison. Building the
cutoff in UTC instead would skew it by the machine's offset. Rows written in one timezone
and pruned in another are off by that difference — accepted, since the error is bounded by
hours against windows measured in days.

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
