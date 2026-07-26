# Upstream Adoption Review

Reviewed 2026-07-26 against upstream [`tover0314-w/opentypeless`](https://github.com/tover0314-w/opentypeless).

## Divergence

| | |
| --- | --- |
| Fork point | `6a4d88a`, 2026-04-13 |
| Upstream commits we don't have | 144 (tip `b0062ac`, 2026-07-25) |
| Our commits upstream doesn't have | 46 |
| Upstream churn in `src/` + `src-tauri/` | ~78.5k insertions across 251 files |

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
| 1 | `e2c21d0` | 8 lines of CSS: `html { color-scheme: light }` / `html.dark { color-scheme: dark }` | We have **no** `color-scheme` declaration, and our theme hook toggles exactly `html.dark` (`src/hooks/useTheme.ts:13`). Native `<select>` dropdowns in Settings (STT provider, LLM model, target language, and the new history retention picker) therefore render with light-mode popups in dark theme. Applies verbatim. |
| 2 | `fc34864` | One pooled `reqwest::Client` in Tauri state instead of per-call construction | We have 11 `reqwest::Client::new()` sites, including one on the dictation hot path (`pipeline.rs`). Each rebuilds a connection pool and re-does the TLS handshake. Straight latency win on the path we just spent two PRs optimizing. |
| 3 | `ca20074` | 22 lines in `lib.rs`: detect NVIDIA + Wayland, disable the DMA-BUF renderer | Fixes a blank-window class of bug on a common Linux configuration. Self-contained, no API surface. |
| 4 | `99a63e0` | Capsule window config + `useCapsuleResize` change so the capsule can't take focus from the paste target | Directly adjacent to our paste-landing work. Worth checking whether our macOS `NONACTIVATING_PANEL` path already covers it — the Windows/Linux side probably isn't. |

## Tier 2 — worth adopting, real work

- **Exponential-backoff retry for providers** (`3689106` STT, `6855c54` LLM, helper in
  `7996aee`). We have no retry anywhere in `src-tauri/` — a transient 429 or 502 from
  Deepgram/AssemblyAI/OpenRouter currently fails the whole dictation. This is the single
  biggest reliability gap the review turned up. Upstream's version is entangled with their
  `AppError`/`UserError` type and their cloud providers; port the retry helper and wire it
  into our three providers rather than taking the diff.
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

## Suggested order

1. Tier 1 #1 (`color-scheme`) — do it with, or right after, the history-retention PR, since
   that PR adds another native `<select>`.
2. Tier 1 #2 (pooled HTTP client) and Tier 2 retry/backoff together — both live in the
   provider layer, and retry is much less useful without connection reuse.
3. Tier 1 #3/#4 opportunistically.
4. Decide the Windows question before spending anything on Tier 2's Windows group.
