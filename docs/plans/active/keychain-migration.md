# Keychain Migration — API Keys Out Of `settings.json`

Written 2026-07-26 as a handoff brief. Nothing implemented yet.

## Goal

`AppConfig.stt_api_key` and `AppConfig.llm_api_key` are plain `String`s
(`storage/mod.rs:12,15`) persisted to `settings.json` by `tauri-plugin-store`. For a fork whose
whole position is BYOK and local-first, provider keys sitting in cleartext on disk is the
biggest remaining gap between what we claim and what we do. Move them into the OS credential
vault — macOS Keychain, Windows Credential Manager, Linux Secret Service — and clear them from
the config file on first run.

This is the highest-value item left on the [upstream adoption list](upstream-adoption.md);
upstream has already closed it and their shape is worth reading.

## Read first, port nothing

Upstream `09a5ff4` → `src-tauri/src/credentials.rs` (546 lines, including its own test module).
Shape worth copying:

- `SERVICE_NAME` + an account suffix, `STORED_CREDENTIAL_VERSION: u8` on the stored payload
  (cheap forward compatibility — take this).
- `CredentialVault` / `CredentialSecretReader` / `CredentialSecretRemover` traits with
  `SystemCredentialVault` as the real implementation. **The traits are the load-bearing part**:
  their test module swaps in a `MemoryVault`, which is the only way `cargo test` can run on
  three OSes without touching a developer's real Keychain. Design for that from the start.
- `migrate_legacy_config_secrets` — vaults both keys and clears them from the config, with a
  test named `migrates_plaintext_api_keys_and_clears_config_after_success`. That "after
  success" is the important ordering: never clear the plaintext until the vault write is
  confirmed, or a vault failure silently destroys the user's key.
- `resolve_config_secret` / `resolve_stt_config_secret` / `resolve_llm_config_secret` and
  `stt_credential_provider(config)` — note the last one implies **per-provider** credentials.

Their `AppConfig` is much larger than ours and the file is entangled with scenes and cloud
auth, so read it for the shape and write ours against our own config.

Dependency, per platform (upstream's, verify current versions):

```toml
keyring = { version = "3.6.3", default-features = false, features = ["apple-native"] }        # macOS
keyring = { version = "3.6.3", default-features = false, features = ["windows-native"] }      # Windows
keyring = { version = "3.6.3", default-features = false, features = [
  "linux-native-sync-persistent", "crypto-rust" ] }                                          # Linux
```

Linux deliberately avoids a D-Bus/Secret-Service runtime requirement this way. Confirm that
still holds, and that `cargo audit` (the non-blocking `audit` CI job) stays quiet.

## The decision that shapes everything else

**Does the webview ever see a key again after it is saved?**

Today it does, constantly. `src/stores/appStore.ts:50,53` hold the keys inside the config
object; `SttPane.tsx:55` and `LlmPane.tsx:107` bind `<input value={config.*_api_key}>`
directly; and the Tauri commands take the key **as a parameter** — `test_stt_connection`,
`bench_stt_connection`, `test_llm_connection`, `bench_llm_connection`, `fetch_llm_models` all
receive `api_key: String` from the frontend (`lib.rs`).

Two coherent designs:

| | A — write-only (recommended) | B — encrypted-at-rest only |
| --- | --- | --- |
| On save | frontend sends the key once; Rust vaults it | same |
| On load | frontend gets a masked placeholder, never the secret | Rust reads the vault and hands the key back into the config |
| Test/bench commands | drop the `api_key` parameter; read from the vault in Rust by namespace + provider | unchanged |
| Buys | the secret stops living in webview memory, store snapshots, and any future config export | encryption at rest only |
| Costs | touches the 5 commands above and both Settings panes; "is a key set?" becomes its own signal | small |

A is the one worth doing — B leaves the secret in the same places minus the file. A also
composes with the existing `Settings` unsaved-changes bar, which compares against the
backend's config: a masked field must not read as an edit (that bug already bit once, see the
`0.5.0` entry in `CHANGELOG.md`).

Decide before writing code, because A changes the Tauri command surface and B doesn't.

## Other decisions

- **Per-provider or single key?** Our config has one `stt_api_key` regardless of the selected
  provider, so switching providers today overwrites it. Upstream keys credentials by
  `(namespace, provider)`, which means switching back remembers the old key. Per-provider is
  better UX and barely more code, but the migration has to pick a provider to file the legacy
  key under — the one currently selected.
- **What happens when the vault read fails at dictation time?** Surface it; do not silently
  fall back to an empty key, which the pipeline reports as "API key is empty". Note the retry
  work already treats auth failures as fatal-on-first-attempt (`retry.rs`), so a missing key
  surfaces immediately rather than after three tries.
- **Onboarding** reads `initial_config.stt_api_key.is_empty()` (`lib.rs:1392`) to decide
  whether to show first-run setup, and `pipeline.rs:537` gates the dictation on the same
  emptiness check. Both need a "has a key" predicate that doesn't require the secret itself.
- **Logging.** `pipeline.rs:532` logs `stt_api_key.len()`. Keep length-only logging, never the
  value, and check nothing new starts logging the secret.

## Scope

| File | Work |
| --- | --- |
| `src-tauri/src/credentials.rs` (new) | Vault traits, `SystemCredentialVault`, versioned payload, migration, resolvers |
| `src-tauri/src/storage/mod.rs` | `stt_api_key` / `llm_api_key` leave `AppConfig`, or become a "set / not set" marker |
| `src-tauri/src/lib.rs` | Run the migration at startup; the 5 commands stop taking `api_key`; onboarding predicate |
| `src-tauri/src/pipeline.rs` | Resolve secrets from the vault when building `SttConfig` / `LlmConfig` (lines 537, 551, 763, 765) |
| `src/stores/appStore.ts`, `src/components/Settings/SttPane.tsx`, `LlmPane.tsx` | Masked field + "key is set" state; stop passing keys to commands |
| `docs/architecture/storage.md` | **Required** by CLAUDE.md rule 1 — config fields and where secrets live |

## Verification

```bash
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
npx tsc --noEmit && npx eslint src/ && npx prettier --check src/ && npx vitest run
typos
```

CI runs all of this on Windows / macOS / Linux and is reliable now (see
[commands.md](../../references/commands.md)).

Tests must go through the trait with an in-memory fake. A test that touches the real vault
will pass locally and behave unpredictably on CI runners.

The manual pass that actually matters, and cannot be automated:

1. Launch with an existing plaintext `settings.json` → keys migrate, dictation still works,
   **and the plaintext is gone from the file**.
2. Launch again → no re-migration, no prompt, keys still resolve.
3. Enter a fresh key in Settings → saved, works, never written to `settings.json`.
4. On macOS, confirm the entry in Keychain Access and that no repeated authorization prompt
   appears on each launch.

## Gotchas

- **Do not run a blanket `cargo fmt`** — format only what you touch.
- **PR-only on `main`**, and `gh pr create` needs `--repo azhurb/opentypeless` (the `upstream`
  remote makes gh target Tover0314's repo otherwise).
- **Do not commit version bumps** to `package.json` / `tauri.conf.json` / `Cargo.toml`.
- A `0.6.0` fold before starting is worth considering: ten commits are unreleased, and this
  work rewrites how `storage/mod.rs` persists secrets, which is exactly the bisect line the
  adoption review argued for.
- `keyring` adds a native dependency per platform. Check the bundle still builds on all three
  in CI before going deep.

## Not this

Apple Speech (`stt/apple_speech.rs`), scenes, and the `lib.rs` module split are separate
items on the [adoption list](upstream-adoption.md). Keep this PR to credentials.
