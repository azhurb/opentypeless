# Storage

OpenTypeless uses local app data for config, history, dictionary, and window/onboarding state, plus the OS credential vault for provider API keys. See [Feature map](../domain/features.md) and [Pipeline](pipeline.md) for how stored values feed user-facing behavior.

Evidence: `src-tauri/src/storage/mod.rs`, `src-tauri/src/credentials.rs`, `src-tauri/migrations/001_init.sql`, `src/lib/tauri.ts`, `src/lib/credentials.ts`, `src/App.tsx`.

## Where secrets live

**API keys are never written to `settings.json`, and never sent to the webview.** They live
in the OS credential vault — see [Credentials](#credentials-os-credential-vault). Everything
else on this page is non-secret configuration.

## Config (`tauri-plugin-store`)

- File: `settings.json` in the OS app-data directory.
- Keys in that file: `app_config` (Rust `storage::AppConfig`), `window_state`, `onboarding_completed` (set from the frontend).
- Manager: `ConfigManager` caches the deserialized config in memory and writes updates back to the store.
- `AppConfig` holds no secrets. It carries `stt_provider` / `llm_provider`, which are also the keys under which credentials are filed.

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

`ConfigManager::load` runs two migrations on the raw JSON value before deserializing into `AppConfig`. Both are idempotent, and a mutated value is written back on the same load.

- `migrate_legacy_config` — pre-multi-language installs persisted `stt_language: String` (with the sentinel `"multi"`). Converts `"multi"` / `""` to `stt_languages = []` and any other code to `[code]`, then removes the legacy key.
- `credentials::migrate_legacy_config_secrets` — moves plaintext `stt_api_key` / `llm_api_key` into the credential vault. Covered under [Credentials](#credentials-os-credential-vault).

Add new migrations to `migrate_legacy_config` rather than re-mapping fields downstream; tests live in `storage::config_migration_tests` and `credentials::tests`.

### Load failures fail open, loudly

If `app_config` cannot be deserialized, `ConfigManager::load` falls back to
`AppConfig::default()` — the app has to start. That fallback re-enables anything the user
opted out of, including `history_enabled`, and if the value also needed legacy migration the
defaults are then written back over their settings. It therefore logs at `error` with the
serde message. If you add another privacy-relevant flag, this is the fail-open path to think
about.

## Credentials (OS credential vault)

Provider API keys live in `src-tauri/src/credentials.rs`, backed by the `keyring` crate:
macOS Keychain, Windows Credential Manager, Linux Secret Service. Before this, they were
plain strings in `settings.json` — for a BYOK, local-first app that was the widest gap
between what the README claims and what the app did.

- **Service name**: `com.opentypeless.app` (matches the `tauri.conf.json` bundle identifier,
  so entries are attributable in Keychain Access / Credential Manager).
- **Account**: `<namespace>:<provider>` — `stt:deepgram`, `llm:openrouter`. Changing this
  format orphans every entry a previous version wrote.
- **Payload**: JSON `{ "version": 1, "secret": "…" }`. The version stamp is forward
  compatibility only; a bare (hand-written) secret is also accepted on read.

### Credentials are per provider

Keys are filed under `(namespace, provider)`, not per namespace. Switching STT provider and
switching back remembers the earlier key instead of overwriting it, and `siliconflow` — which
is both an STT and an LLM provider id — gets two independent slots.

### Keys are write-only from the webview's perspective

A secret travels one way: the user types it, `set_api_key` puts it in the vault, and nothing
ever hands it back. `AppConfig` has no key fields, the Zustand store has no key fields, and
`get_config` returns no secret. What the frontend can ask is *whether* a key exists, via
`get_credential_status(sttProvider, llmProvider) -> { stt: bool, llm: bool }`.

Consequences worth knowing:

| Concern | How it works |
| --- | --- |
| Settings / onboarding input | The field is genuinely **empty** with a "saved" placeholder — not a masked value. `keyDrafts[ns] === null` means untouched, so the unsaved-changes bar cannot mistake a placeholder for an edit (the `0.5.0` bug in `CHANGELOG.md`). |
| Removing a key | "Remove" stages an empty-string draft; Save calls `set_api_key` with `""`, which deletes the entry. It is a pending change like any other setting, not an immediate side effect. |
| Testing a key | `test_*` / `bench_*` / `fetch_llm_models` take `api_key: Option<String>`. `Some(candidate)` probes an unsaved key — required by onboarding, where nothing is saved yet, and by Settings, where probing the stored key right after pasting a new one would report on the wrong credential. `None` means "use the vault". A candidate is never persisted as a side effect of testing. |
| `fetch_llm_models` | Gained a `provider` parameter, purely to name the vault entry to fall back on. |
| Onboarding gate | `should_show_window_on_launch` takes "the vault has no entry for the selected STT provider" instead of `stt_api_key.is_empty()`. An unreadable vault counts as no key, erring toward showing onboarding rather than starting hidden and broken. |
| Logging | `pipeline.rs` logs key **length** only. Never log the value. |

### Legacy plaintext migration

`migrate_legacy_config_secrets` runs inside `ConfigManager::load`. For each of
`stt_api_key` / `llm_api_key` it writes the secret to the vault under the currently selected
provider, then removes the plaintext field.

**The ordering is load-bearing: the plaintext is cleared only after the vault write returns
`Ok`.** A locked, unavailable, or denied vault leaves the config exactly as it was, so the
user keeps a working key and the migration retries next launch. Clearing first would destroy
the only copy of a secret the user may never have written down.

Because `AppConfig` no longer models those fields, serializing it would drop them — so a
launch with a locked vault followed by *any* Settings save would erase the key anyway.
`ConfigManager` therefore holds `pending_legacy_secrets`: whatever the migration could not
vault is re-attached by every `save` until a later launch succeeds.

Other cases: an empty legacy field is dropped without touching the vault; an existing vault
entry wins over stale plaintext (and the plaintext is dropped); a missing `*_provider` leaves
the plaintext in place, since there is nothing to file it under.

### Reads are cached for the session

`CachingVault` wraps the real vault and remembers secrets it has already read,
so a session touches the OS credential store roughly twice instead of twice per
dictation (the pipeline resolves an STT key and an LLM key every time).

This is a macOS usability fix. A Keychain prompt offers Deny / Allow / **Always
Allow**, and plain "Allow" grants exactly one access — so without the cache, a
user who did not pick "Always Allow" was re-prompted on every dictation, which
reasonably reads as something malicious.

Only successful reads are cached. Errors are not, so a locked keychain keeps
reporting itself instead of being remembered as a failure for the session;
misses are not, so a key added out of band is still picked up. `write` and
`delete` update the cache after the store accepts the change, never before.

### When macOS actually prompts

The Keychain ACL matches on the app's **designated requirement**, which differs
by how the build is signed:

| Build | Designated requirement | Prompt behavior |
| --- | --- | --- |
| Release (`.github/workflows/release.yml` imports the "OpenTypeless Release" cert) | `identifier "com.opentypeless.app" and certificate leaf = H"…"` | Stable across versions — an update does **not** re-prompt |
| Local `npm run tauri build` (no cert) | `cdhash H"…"` | Changes every rebuild, so each local rebuild prompts once |

So repeated prompts while developing are expected and are not a defect. They
would only reach users if the release signing certificate were rotated.

Windows and Linux have no equivalent per-app prompt: Credential Manager is
scoped to the user account, and Secret Service unlocks with the login session.
**Needs confirmation**: behavior on a Linux box with no Secret Service provider
at all (minimal WM, headless) — see the open follow-up in
[`../plans/active/credential-vault-followups.md`](../plans/active/credential-vault-followups.md).

### Vault errors are not "no key"

`CredentialVault::read` returns `Ok(None)` for "no entry" and `Err` for "could not reach the
vault". Collapsing the two turns a locked keychain into the pipeline's misleading "API key is
not configured", which sends the user to re-enter a key that is already there. The STT path
surfaces a distinct message and aborts; the LLM path logs a warning and skips polish, because
failing the dictation outright would throw away a transcript the user already spoke.

The same distinction reaches the UI. `get_credential_status` returns a three-state
`KeyPresence` per namespace — `saved` / `missing` / `unreadable` — not a boolean. Reporting an
unreadable vault as `missing` renders an empty field, which invites the user to retype the key
or press Remove, destroying a credential that was fine. On macOS that is one declined prompt
away. The `unreadable` state shows an explicit "couldn't read your keychain" message and
hides Remove, since offering to delete a key whose existence is unknown is not a safe option.

### Testing

`CredentialVault` is a trait so tests can substitute `MemoryVault`. **Tests must never touch
the real vault** — CI runs `cargo test` on three OSes, where a real vault either prompts for
authorization or fails on a headless runner. `MemoryVault::failing(msg)` exercises the
vault-rejects-the-write path.

The Linux build needs `libdbus-1-dev` (installed by both workflows in `.github/workflows/`);
the `linux-native-sync-persistent` feature uses kernel keyutils for the session and Secret
Service for persistence across reboots.

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
