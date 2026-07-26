# Upstream Adoption Review

Reviewed 2026-07-26 against upstream [`tover0314-w/opentypeless`](https://github.com/tover0314-w/opentypeless). Status re-checked 2026-07-26 after the provider-layer PRs landed.

## Divergence

| | |
| --- | --- |
| Fork point | `6a4d88a`, 2026-04-13 |
| Upstream commits we don't have | 144 (tip `b0062ac`, 2026-07-25) — unchanged since the review; upstream has not pushed since |
| Our commits upstream doesn't have | 59 |
| Unreleased on our `main` | 10 commits since `v0.5.0` |
| Upstream churn in `src/` + `src-tauri/` | ~78.5k insertions across 251 files |

## Status at a glance

Adoption decisions and where each stands. Nothing here is blocked on upstream.

| Item | Decision | Status |
| --- | --- | --- |
| Tier 1 #1 — `color-scheme` for native selects | Adopt | **Landed** (#26) |
| Tier 1 #2 — pooled `reqwest::Client` | Adopt | **Landed** (#31) |
| Second pass #3 — quota-free OpenAI connection test | Adopt | **Landed** (#31) |
| Tier 2 — provider retry with backoff | Adopt, rewrite not port | **Landed** (#31), see [`../completed/provider-retry.md`](../completed/provider-retry.md) |
| Second pass #1 — keychain migration | Adopt, rewrite not port | **Landed** (#36), see [`../completed/keychain-migration.md`](../completed/keychain-migration.md) |
| Second pass, small — `3f9fbc2` preserve STT provider errors | Adopt | Open. Same code path as the retry work; deliberately not folded in |
| Second pass #2 — Apple Speech on-device STT | Adopt as its own feature | Open |
| Tier 1 #3 — NVIDIA/Wayland DMA-BUF workaround | Adopt | Open |
| Tier 1 #4 — capsule focus-steal fix | Adopt after checking our `NONACTIVATING_PANEL` path | Open |
| Second pass, small — `dfb8ab8` cancel-crash, frontend half only | Adopt partially | Open |
| Tier 2 — Windows output correctness (3 commits) | Blocked on a product decision | Open — decide whether we support Windows properly first |
| Tier 2 — Linux packaging fixes (6 commits) | Adopt when needed | Deferred to the next Linux release |
| Tier 2 — unsigned-app install docs | Adopt | Open, README gap |
| Tier 3 — scenes, settings backup, local Whisper, Qwen3 ASR, `lib.rs` split | Concept only, not ports | Open, unscheduled |
| i18n — 8 extra locales | Partial value only | Open; a translation project, not a cherry-pick |
| `selection.rs` | **Rejected** | Would reintroduce the `osascript` path PR #7 removed |
| Cloud STT/LLM, auth/OAuth, subscriptions, quota, "ask anything", Vercel, funding link | **Rejected** | The product direction this fork exists to avoid |

Not in the original review, found while implementing the retry work — both fixed, neither upstream-derived:

| Item | Status |
| --- | --- |
| `stt::create_provider` had no `deepgram` arm since the initial commit; the UI offered it and it silently fell back to GLM-ASR | **Landed** (#33) |
| Deepgram discarded the transcript on `speech_final`; both streaming providers closed without draining the provider's flush | **Landed** (#33) / **open PR** (#34, needs live validation) |

**Upstream's direction has split from ours.** The bulk of those 78.5k lines is a managed
cloud product: auth and OAuth deep links (`authStore.ts` +419, `desktop-auth-callback.ts`),
subscriptions and entitlements (`subscription-refresh-policy.ts`, "Pro benefits",
`upgradeBenefits`), server-side quota accounting, `stt/cloud.rs` / `llm/cloud.rs` providers,
managed recording limits, and a Vercel deployment. Our fork's stated position is BYOK-only
with no auth, subscription, telemetry, or auto-update code in the build
([Feature map](../../domain/features.md#privacy-and-local-first-byok)), so none of that is
adoptable — and reviewing it is wasted effort beyond confirming what it is.

Upstream did **not** drop BYOK; the cloud providers sit alongside the direct ones. That
means their provider-layer and platform fixes are still cherry-pickable.

## Tier 1 — small, self-contained, aligned

Ranked by value per line of change.

| # | Upstream | What | Why it matters here |
| --- | --- | --- | --- |
| 1 | `e2c21d0` | 8 lines of CSS: `html { color-scheme: light }` / `html.dark { color-scheme: dark }` | **Landed** (#26). |
| 2 | `fc34864` | One pooled `reqwest::Client` in Tauri state instead of per-call construction | **Landed** — see [`../completed/provider-retry.md`](../completed/provider-retry.md). |
| 3 | `ca20074` | 22 lines in `lib.rs`: detect NVIDIA + Wayland, disable the DMA-BUF renderer | Fixes a blank-window class of bug on a common Linux configuration. Self-contained, no API surface. |
| 4 | `99a63e0` | Capsule window config + `useCapsuleResize` change so the capsule can't take focus from the paste target | Directly adjacent to our paste-landing work. Worth checking whether our macOS `NONACTIVATING_PANEL` path already covers it — the Windows/Linux side probably isn't. |

## Second pass — three items the first pass missed

The first pass ranked by commit subject, which hid the best finds. `09a5ff4`, titled
`feat: prepare v1.1.48 release`, is **not** a version bump: 103 files, +25,348 lines. It
introduces `credentials.rs`, `commands/credentials.rs`, `stt/apple_speech.rs`,
`native_hotkey.rs`, `selection.rs`, and scenes in one squash. Nothing inside it is
cherry-pickable — these are read-and-reimplement, not `git cherry-pick`.

| # | Upstream | What | Why it matters here |
| --- | --- | --- | --- |
| 1 | `09a5ff4` → `credentials.rs`, `commands/credentials.rs`, `keyring` 3.6.3 | API keys move from the config file into the OS credential vault (macOS Keychain / Windows Credential Manager / Linux Secret Service), behind `CredentialVault` / `CredentialSecretReader` / `CredentialSecretRemover` traits | We store `stt_api_key` and `llm_api_key` as plaintext `String`s in `AppConfig`, persisted to `settings.json` by `tauri-plugin-store` (`src-tauri/src/storage/mod.rs:12,15`). For a fork whose whole position is BYOK and local-first, keys sitting in cleartext on disk is the most on-mission gap upstream has already closed. `migrate_legacy_config_secrets` is exactly the migration we'd need — it vaults both keys and **clears them from the config**, with a test (`migrates_plaintext_api_keys_and_clears_config_after_success`). Linux uses `linux-native-sync-persistent` + `crypto-rust`, so no D-Bus requirement. |
| 2 | `09a5ff4` → `stt/apple_speech.rs` | Apple `SFSpeechRecognizer` as a normal STT provider: 684 lines, pure Rust `objc2` `msg_send!`, no Swift and no build script | On-device (`setRequiresOnDeviceRecognition: true` whenever `supportsOnDeviceRecognition`), no API key, no cost, no network. Serves [Offline And Local Models](../../domain/features.md#offline-and-local-models) at a fraction of the size of local Whisper or Qwen3, which Tier 3 does list. Ships a real availability/authorization model (`AppleSpeechAvailability` with `issue_code`) rather than a bare provider. macOS only. |
| 3 | `da7b5fd` | 22 lines in `commands/stt.rs`: don't spend OpenAI Whisper quota to run "Test connection" | **Landed** — folded into the provider-retry PR; see [`../completed/provider-retry.md`](../completed/provider-retry.md). |

Two more of the same kind, smaller:

- `3f9fbc2` **preserve STT provider errors** (89 lines, `pipeline.rs`) — surface the provider's
  actual error instead of collapsing it into a generic failure. BYOK users debug their own
  keys, endpoints, and quotas, so the real message is the whole value.
- `dfb8ab8` **prevent recording cancel crashes** — take only the frontend half
  (`CapsuleRecording` / `CapsuleProcessing` cancel buttons + tests); the Rust half is in
  `stt/cloud.rs` and doesn't apply.

**Do not adopt `selection.rs` as-is.** It copies the selection by shelling out to
`osascript` → System Events, the exact path PR #7 removed so that macOS needs only the
Accessibility grant and never Automation.

## Tier 2 — worth adopting, real work

- ~~**Exponential-backoff retry for providers**~~ (`3689106` STT, `6855c54` LLM, helper in
  `7996aee`). **Landed** — written against `anyhow` rather than porting upstream's
  `AppError`/`UserError` entanglement, and scoped to the calls where a second attempt is
  actually safe. See [`../completed/provider-retry.md`](../completed/provider-retry.md).
- **Windows output correctness** (`b08618c` SendInput source, `9063f1a` modifier guard,
  `dc38c38` hotkey hook with module handle). Our `src-tauri/src/output/` has no
  `keyboard.rs`, `windows_sendinput.rs`, or `windows_modifier_guard.rs` at all, and there is
  no `native_hotkey.rs`. Whether this matters depends on whether we intend to support
  Windows properly; if yes, these three are the ones to take, and they come with a spec doc.
- **Linux packaging fixes** (`4c7caf5`, `cb4924f`, `025aed0`, `e2a134e`, `8447244`,
  `00534e6`): AppImage Wayland ABI conflicts, versioned Wayland client exclusion, DirIcon
  and icon mapping. Relevant only when we next cut a Linux release.
- **`docs`: install instructions for an unsigned app** (`e074196`) — our README has the same
  gap for macOS Gatekeeper and Windows SmartScreen.

## Tier 3 — feature ideas, not ports

- **Scenes** (`src/lib/scenes/`): named prompt presets — clean dictation, meeting notes,
  professional email — selectable per dictation, with import/export. BYOK-compatible and a
  natural fit for our polish step. Adopt the concept; the code assumes their much larger
  `AppConfig`.
- **Settings backup/restore** (`src/lib/backup-settings.ts`): export/import config as JSON
  with API keys deliberately excluded. Same story — good idea, their implementation is bound
  to scenes, voice routing, and shortcut bindings we don't have.
- **Local Whisper STT** (`fe07da7`) and **Qwen3 ASR** (`749d289`, `b676291`): more BYOK/local
  provider coverage, which is on-mission for [Offline And Local
  Models](../../domain/features.md#offline-and-local-models). Each is ~600–2200 lines
  including their 10 locale files.
- **Extract `commands/`, `tray`, `hotkey` modules out of `lib.rs`** (`2d1f77c`, `94dfd22`,
  `ad4f841`). Our `lib.rs` is ~1400 lines and holds commands, tray, hotkey parsing, macOS
  FFI, and setup. The refactor is sound, but taking it as a diff would conflict with
  everything we've changed there. Reference it if we do our own split.

## i18n: partial win only

Upstream now ships 10 locales (`de`, `es`, `fr`, `it`, `ja`, `ko`, `pt`, `ru` added to
`en`/`zh`), ~860 lines each. Key-set comparison against our `en.json`:

| | |
| --- | --- |
| Our keys | 173 |
| Upstream keys | 758 |
| Shared | 91 |
| Ours only | 82 (capsule tips, correction toasts, clipboard tip, app shell) |
| Upstream only | 667 (cloud, auth, scenes, ask-anything, quota) |

So the 8 locale files would translate ~53% of our strings and carry 667 keys of dead
weight. Adopting means extracting the 91 shared keys from each file and translating the
remaining 82 ourselves — worthwhile if we want more languages, but it's a translation
project, not a cherry-pick.

## Not adoptable

Managed cloud STT/LLM, auth and OAuth deep-link flow, subscriptions/entitlements/quota sync,
"ask anything" cloud flow, Vercel deployment, managed recording limits, Buy Me a Coffee
funding link. These implement the product direction our fork exists to avoid.

## Release first, then adopt

**Cut 0.5.0 before pulling anything in.** As of 2026-07-26 exactly one commit sits after the
`v0.4.0` tag — `2628837` (#20, history toggle + retention) — and it carries three user-facing
`[Unreleased]` entries, including the macOS-only "Clear All History did nothing" fix. Reasons
to ship that first rather than after:

- **Bisectability.** The two highest-value adoptions land in the same places #20 just changed:
  Second pass #1 rewrites how `AppConfig` persists secrets (`storage/mod.rs`, the file #20's
  retention config lives in), and Tier 2 retry touches the provider layer. A `v0.5.0` tag
  draws the line between our own work and upstream-derived work, so a later regression is one
  bisect instead of an argument.
- **Release CI needs attention of its own.** Tag-push triggers stall on this fork and need
  babysitting; don't debug the release pipeline and freshly-ported foreign code in the same
  sitting.
- **#20 is verified, the adoptions aren't.** Shipping the tested thing now costs one fold PR
  plus a tag.

Gate: finish the manual pass on #20's history toggle and retention picker (a `tauri dev`
build was running against `2628837` for this). Then the documented fold-before-tag ordering —
fold `[Unreleased]` into `[0.5.0]` in its own PR, then tag.

## Suggested order

1. ~~Fold `[Unreleased]` → `[0.5.0]`, tag, release.~~ **done** — `v0.5.0` is cut. Ten commits
   have accumulated since; a `0.6.0` fold is available whenever wanted. The keychain work
   has now landed on top of `storage/mod.rs` without a fold, so the bisect line the review
   argued for is not there — worth weighing before the next storage-shaped change.
2. ~~Tier 1 #1 (`color-scheme`)~~ **done** (#26).
3. ~~Second pass #3 (`da7b5fd`, quota-free connection test) and #1 (keychain migration)~~
   **both done** — the two on-mission BYOK items. #1 landed in #36 with the
   `docs/architecture/storage.md` update; see
   [`../completed/keychain-migration.md`](../completed/keychain-migration.md) for where the
   implementation diverged from the brief.
4. ~~Tier 1 #2 (pooled HTTP client) and Tier 2 retry/backoff together~~ **done** — both lived
   in the provider layer, and retry is much less useful without connection reuse. Second pass
   `3f9fbc2` (preserve provider errors) was *not* folded in; still open on the same code path.
5. Second pass #2 (Apple Speech) as its own feature PR — an on-device, key-free provider is
   the most visible user-facing win in the list.
6. Tier 1 #3/#4 opportunistically.
7. Decide the Windows question before spending anything on Tier 2's Windows group.
