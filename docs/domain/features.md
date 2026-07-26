# Feature Map

Reconciles public feature descriptions with what the repository actually does. Each entry lists the user-facing promise, the code that backs it, and any mismatch.

Related: [Providers](../architecture/providers.md) for STT/LLM IDs, [Pipeline](../architecture/pipeline.md) for runtime behavior, [Voice input](voice-input.md) for prompt rules.

Sources: `README.md`, current repo code. This fork is BYOK-only; the upstream cloud subscription, account/auth flow, and scene packs have been removed.

## Bring Your Own Providers

User-facing promise: users can configure their own STT and LLM providers with local API keys and switch providers from Settings.

Repo evidence:

- STT provider options live in `src/lib/constants.ts` and `src/stores/appStore.ts`.
- STT provider factory lives in `src-tauri/src/stt/mod.rs`.
- LLM provider options live in `src/lib/constants.ts` and `src/stores/appStore.ts`.
- LLM provider factory lives in `src-tauri/src/llm/mod.rs`.
- Connection tests and latency benchmarks live in `src-tauri/src/lib.rs`.

Current repo STT provider labels (from `src/lib/constants.ts`):

- Deepgram Nova-3 — present in the dropdown but **not registered in `stt::create_provider`**; selecting it currently falls through to the GLM-ASR default. See [Providers → mismatches](../architecture/providers.md#mismatches-with-the-frontend-list).
- AssemblyAI
- GLM-ASR
- OpenAI Whisper
- Groq Whisper
- SiliconFlow

Current repo LLM provider labels:

- Zhipu
- DeepSeek
- SiliconFlow
- OpenAI
- Google Gemini
- Moonshot
- Qwen
- Groq
- Claude
- Ollama
- OpenRouter

Needs confirmation:

- The website mentions `GLM-4-Voice`; repo code uses `glm-asr` (model `glm-asr-2512`).
- The website mentions Yi and LM Studio; provider constants do not list them. LM Studio may work through a custom OpenAI-compatible base URL — inference, not verified.

## AI-Powered Text Polishing

User-facing promise: after transcription, the app can refine text by adding punctuation, removing filler words, improving formatting, preserving key terms, and optionally translating.

Repo evidence:

- `polish_enabled`, `translate_enabled`, and `target_lang` are part of `AppConfig`.
- LLM calls are orchestrated in `src-tauri/src/pipeline.rs`.
- Prompt construction lives in `src-tauri/src/llm/prompt.rs`.
- The LLM Settings pane exposes provider, model, base URL, AI polish, translation, and selected-text context controls.

Current prompt behavior includes:

- punctuation cleanup, with the polished text required to end with terminal punctuation (`. ? !` or the language equivalent)
- minimal-edit polishing: small grammar fixes only, no rephrasing, restructuring, or word reordering — the user should still recognize their dictated sentences
- filler-word removal
- list and paragraph formatting
- dictionary term preservation
- selected-text instruction mode
- translation to configured target language
- prompt-injection resistance for transcript and selected text

A single trailing space is appended to whatever is pasted into the foreground app (see [Pipeline](../architecture/pipeline.md)), so successive dictations don't glue together.

Output is always delivered via the system clipboard plus a synthesized Cmd+V (Ctrl+V on Windows/Linux). The user's prior clipboard contents are snapshotted and restored after the paste lands. For terminal-hosted CLIs that don't handle bulk pastes well (Claude CLI, Codex CLI, Gemini CLI) the paste is split into smaller chunks with brief inter-chunk delays — see [Pipeline → Output](../architecture/pipeline.md) for the chunking constants and the list of recognised terminal targets.

For Gemini models (any model name containing `gemini`), the LLM request additionally sets `reasoning_effort: "none"` to keep thinking off — `gemini-flash-lite` already defaults to no thinking, but the explicit opt-out is defensive against any future model where it might engage. Other providers silently ignore this field.

Needs confirmation:

- The core polish prompt is compiled in Rust and not directly user-editable in Settings.

## Language Support And Auto Detection

User-facing promise: users mark zero or more languages they expect to speak; the STT auto-detects, the polish prompt is biased toward the marked set, and dictation history shows the detected language per row.

Repo evidence:

- STT languages live in `AppConfig.stt_languages: Vec<String>` (empty = auto-detect).
- `src/lib/constants.ts` exposes 20 language options; the Settings UI renders them as multi-select chips (`src/components/Settings/SttPane.tsx`). Selecting zero languages is the canonical "auto" state — there is no `"multi"` sentinel anymore.
- A one-shot migration in `ConfigManager::load` (`src-tauri/src/storage/mod.rs::migrate_legacy_config`) converts the pre-existing single-value `stt_language` field on disk: `"multi"` and empty become `[]`; any other code becomes `[code]`. After migration the old field is removed.
- Wire mapping (see [Providers → Language hint mapping rule](../architecture/providers.md#language-hint-mapping-rule)): Whisper-compatible adapters pin a `language=` form field only when exactly one is selected; Deepgram pins via URL or falls back to its native `multi` mode.
- Detected language is captured from STT responses and threaded into the polish prompt + history + a `pipeline:timing.detected_language` event (see [Pipeline → Detected language threading](../architecture/pipeline.md#detected-language-threading)).
- `src-tauri/src/llm/prompt.rs` injects a one-line context clause when detected language is known and lists the user's configured set; both pass through a strict display-name map to avoid prompt injection from wire-supplied values.
- A rate-limited toast (`src/hooks/useDetectedLanguageNotifier.ts`) tells the user when the STT detected a language not in their set; cooldown is 10 s per language code.

Needs confirmation:

- Whether GLM-ASR and SiliconFlow report `language` under `response_format=verbose_json`. Both currently accept the field silently; the parser falls back to no badge if absent.

## Global Hotkey

User-facing promise: a global shortcut starts voice input from other desktop apps.

Repo evidence:

- Hotkey config lives in `AppConfig.hotkey` and `AppConfig.hotkey_mode`.
- Defaults are `Alt+/` on macOS and `Ctrl+/` elsewhere.
- `parse_hotkey()` and `build_shortcut_handler()` live in `src-tauri/src/lib.rs`.
- Settings can pause/resume hotkey handling while capturing a new shortcut.

Current behavior:

- `hold` mode starts on key press and stops on key release.
- `toggle` mode starts and stops on repeated shortcut activation.

Needs confirmation:

- The website text emphasizes toggle behavior, while the repo default is hold mode.

## Custom Dictionary

User-facing promise: users can add specialized terms so output preserves exact spelling.

Repo evidence:

- Dictionary storage lives in `DictionaryStore`.
- Dictionary UI lives under `src/components/Settings/DictionaryPane.tsx`.
- Dictionary words are loaded before recording in `src-tauri/src/pipeline.rs`.
- Prompt construction injects sanitized dictionary terms in `src-tauri/src/llm/prompt.rs`.

Needs confirmation:

- Pronunciation is stored in the dictionary schema/UI, but current prompt construction uses only words from `DictionaryStore::words()`.

### Learn From Corrections (macOS, experimental)

Off by default. When enabled (Settings → Dictionary → "Learn from corrections"), OpenTypeless watches the focused text field via macOS Accessibility for up to 60 s after each dictation. If the user replaces exactly one word inside the typed span with a proper-noun-shaped replacement (capitalized, camelCase, all-caps acronym, or alphanumeric brand), the new word is added to the dictionary optimistically and a 5 s Undo toast appears in the capsule overlay. The toast reads *Replaced "Vladislav" with "Vlad"* — showing both the STT-produced word and the user's correction. The watcher skips `AXSecureTextField` (password fields) and never logs field values.

"Edit done" is detected via a **boundary-anchored settle predicate** — the chars flanking the dictated region in the field must still match the original surrounding context before a candidate substitution is considered. This is more robust than a pure time-based debounce, which mis-fires when the user pauses mid-edit. When dictation lands in an empty field (no surrounding context to anchor against), the watcher falls back to time-based debounce. Implementation: `src-tauri/src/correction/{mod,boundary,diff,classify}.rs`; frontend toast in `src/components/Capsule/CorrectionToast.tsx`.

Auto-learned rows in Settings → Dictionary show a subtle Sparkles icon next to the word; hovering surfaces the STT-produced word the user replaced. See [Storage → Dictionary](../architecture/storage.md#dictionary-dictionarystore) for the underlying `source`/`observed_source` columns.

## macOS Permissions UX

User-facing promise: the app surfaces the macOS permissions it needs (Microphone, Accessibility) up front during onboarding, and recovers gracefully when a grant is missing or has been revoked.

Repo evidence:

- Onboarding inserts a `PermissionsStep` on macOS between the LLM and How-It-Works steps (six steps on macOS, five elsewhere) — `src/components/Onboarding/index.tsx` and `src/components/Onboarding/PermissionsStep.tsx`. The step shows current Microphone and Accessibility status and routes the Grant buttons to either `requestMicrophonePermission` / `requestAccessibilityPermission` (when not yet asked) or directly to System Settings (when already denied — the macOS dialog is one-shot per install).
- `src/App.tsx` runs both `checkMicrophonePermission` and `checkAccessibilityPermission` at startup on macOS, and auto-prompts microphone when the status is `not_determined` *and* onboarding has already been completed (so an existing user who upgrades to a build with the upfront prompt still sees the dialog at launch instead of mid-dictation). During onboarding the auto-prompt is suppressed — the `PermissionsStep`'s Grant button owns that moment.
- Main-window banners — `src/components/MainLayout/AccessibilityBanner.tsx` and `src/components/MainLayout/MicDeniedBanner.tsx` — appear on macOS whenever the corresponding store flag turns negative. Each banner has a Grant / Open Settings action and a dismiss button.
- The capsule itself becomes actionable on permission errors: `src/components/Capsule/CapsuleError.tsx` renders localized, sticky messages for `ACCESSIBILITY_REQUIRED` and `MICROPHONE_DENIED` (no 2.5 s auto-clear, unlike transient errors), and `src/components/Capsule/index.tsx` swaps the capsule click handler from "start recording" to "open the relevant System Settings pane" while a permission error is active.
- The pipeline refuses to start when Microphone is `denied` / `restricted` (`pipeline.rs::start`); paste similarly refuses when Accessibility is missing (`pipeline.rs::output_text`). Both emit machine-readable error codes — see [Pipeline → Events](../architecture/pipeline.md#events) and [Frontend ↔ Backend → Events](../architecture/frontend-backend.md#events).
- Window-show predicate at launch lives in `src-tauri/src/lib.rs::should_show_window_on_launch` and surfaces the main window whenever the user is still in onboarding (no STT key yet or `onboarding_completed` missing / false), so a flag drop or a partial-config state always lands the user on a visible flow rather than a tray-only launch.

Troubleshooting flows for the signature-mismatch and one-shot-dialog cases live in [`docs/references/troubleshooting.md`](../references/troubleshooting.md).

## Privacy And Local-First BYOK

User-facing promise: API keys stay local and provider requests go directly to the selected provider. There are no OpenTypeless servers in the loop.

Repo evidence:

- API keys are stored outside `settings.json` (`src-tauri/src/credentials.rs`) and are never returned to the webview: the OS credential store on Windows and Linux, an owner-only file on macOS (where the Keychain would prompt after every update without a paid Apple Developer ID). See [Storage → Credentials](../architecture/storage.md#credentials-os-credential-vault).
- Plaintext keys from older installs migrate into the vault on first launch, and are cleared from `settings.json` only once the vault write succeeds.
- All STT/LLM providers call external provider endpoints directly from Rust.
- There is no auth, subscription, telemetry, or auto-update code in the build.

### Optional History And Retention

User-facing promise: recording dictation history is a choice, and what is recorded does not
have to be kept forever.

Repo evidence:

- Settings → General → History has a `Save dictation history` toggle (`history_enabled`) and
  a `Keep history for` picker (`history_retention_days`: Forever / 7 / 30 / 90 days).
- With the toggle off, the pipeline skips the history insert entirely; dictations are still
  typed. Entries already stored stay listed and searchable, and the History page says so.
- Retention applies to stored entries whether or not saving is on, so turning saving off is
  not a way to freeze the archive; the History notice and the Settings hint both say this.
- Narrowing the retention window asks for confirmation before it deletes, matching
  "Clear All History".
- Rows removed by retention or "Clear all" are scrubbed from the database file, not just
  unlinked — see [Storage → Retention](../architecture/storage.md#retention).
- Both default to today's behavior (on, forever), so upgrading deletes nothing.

## Offline And Local Models

User-facing promise: local LLM use is supported through Ollama. With a local STT provider plus a local LLM, the app can run fully offline.

Repo evidence:

- Ollama is listed as an LLM provider with `http://localhost:11434/v1`.

Needs confirmation:

- The current repo does not expose a clearly named local STT provider. Local Whisper support may depend on OpenAI-compatible endpoints or external setup not documented here.
