# Feature Map

Reconciles public feature descriptions with what the repository actually does. Each entry lists the user-facing promise, the code that backs it, and any mismatch.

Related: [Providers](../architecture/providers.md) for STT/LLM IDs, [Pipeline](../architecture/pipeline.md) for runtime behavior, [Cloud Pro mode](cloud-pro.md) for the `cloud` provider, [Voice input](voice-input.md) for prompt rules.

Sources: [OpenTypeless website feature page](https://www.opentypeless.com/en/features), `README.md`, current repo code.

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
- OpenTypeless Cloud

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
- OpenTypeless Cloud

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

A single trailing space is appended to whatever is typed into the foreground app (see [Pipeline](../architecture/pipeline.md)), so successive dictations don't glue together.

Needs confirmation:

- The website mentions customizing the polish prompt. Current repo evidence shows scene prompt templates can be copied from cloud scene packs, but the core local prompt is compiled in Rust and not directly user-editable in Settings.

## Language Support And Auto Detection

User-facing promise: users can speak in many languages, use auto detection, or choose a preferred language.

Repo evidence:

- STT language setting lives in `AppConfig.stt_language`.
- `src/lib/constants.ts` exposes `Auto Detect` plus 21 language options for STT and target translation.
- `src-tauri/src/pipeline.rs` maps `stt_language == "multi"` to `None` for most provider configuration.
- `src-tauri/src/llm/prompt.rs` maps target-language codes for translation prompt text.
- `README.md` says STT support is provider-dependent and can reach 99+ languages.

Needs confirmation:

- The exact 99-language list is provider-specific and is not stored in this repo.
- Auto-detection behavior depends on the selected STT provider.

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
- Scene packs can merge dictionary terms into the local dictionary.

Needs confirmation:

- Pronunciation is stored in the dictionary schema/UI, but current prompt construction uses only words from `DictionaryStore::words()`.

## Privacy And Local-First BYOK

User-facing promise: in BYOK mode, API keys stay local and provider requests go directly to the selected provider rather than through OpenTypeless servers.

Repo evidence:

- `README.md` states BYOK requests go directly to configured providers.
- API keys are stored in `settings.json` through `tauri-plugin-store`.
- Non-cloud STT/LLM providers call external provider endpoints directly from Rust.
- Cloud providers use OpenTypeless proxy endpoints with a session token.

Needs confirmation:

- The website says keys are stored in an encrypted configuration file. Current repo evidence only confirms `tauri-plugin-store` local storage; encryption is not proven by the inspected code.

## Optional Cloud Pro Mode

User-facing promise: users can select `cloud` providers to avoid managing provider API keys.

Repo evidence:

- `cloud` is present in STT and LLM provider lists.
- `SessionTokenStore` stores the bearer token after frontend auth.
- Connection tests check `/api/subscription/status` and require `plan == "pro"`.
- Cloud STT and LLM providers proxy through `{API_BASE_URL}/api/proxy/stt` and `{API_BASE_URL}/api/proxy/llm`.
- Build-time base URL overrides are `VITE_API_BASE_URL` and `API_BASE_URL`.

Needs confirmation:

- Exact quota and billing rules should be confirmed against backend/product policy.

## Offline And Local Models

User-facing promise: some configurations can run without OpenTypeless cloud dependency, and local LLM use is supported through Ollama.

Repo evidence:

- Ollama is listed as an LLM provider with `http://localhost:11434/v1`.
- BYOK mode does not require OpenTypeless cloud.
- README says local STT plus local LLM can work offline.

Needs confirmation:

- The current repo does not expose a clearly named local STT provider. Local Whisper support may depend on OpenAI-compatible endpoints or external setup not documented here.

## Scene Packs

User-facing promise from repo UI: scene packs provide prompt templates and dictionary terms for specific workflows.

Repo evidence:

- Settings includes a Scenes pane.
- Scenes are fetched from `/api/scenes`.
- Scene packs include `name`, `description`, `category`, `promptTemplate`, `dictionaryTerms`, and `isPro`.
- Users can copy prompt templates and merge scene dictionary terms.

Needs confirmation:

- How copied scene prompts are meant to alter polishing behavior is not wired into local prompt configuration in the inspected code.
