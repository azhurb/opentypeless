# Providers

STT and LLM integrations use trait + factory patterns in Rust. Provider IDs also appear in the frontend Zustand store. The IDs in `appStore.ts` and the match arms in the Rust factories must stay in sync.

Used by: [Pipeline](pipeline.md) (calls `create_provider`), [Feature map](../domain/features.md) (user-facing labels).

Evidence: `src-tauri/src/stt/mod.rs`, `src-tauri/src/llm/mod.rs`, `src/stores/appStore.ts`, `src/lib/constants.ts`, `src/components/Settings/`.

## STT Providers

Trait in `src-tauri/src/stt/mod.rs`:

```rust
async fn connect(&mut self, config: &SttConfig) -> Result<()>;
async fn send_audio(&mut self, chunk: &[u8]) -> Result<()>;
async fn recv_transcript(&mut self) -> Result<Option<TranscriptEvent>>;
async fn disconnect(&mut self) -> Result<DisconnectResult>;
fn name(&self) -> &str;
```

`TranscriptEvent` variants: `Partial`, `Final { text, confidence, language }`, `SpeechStarted`, `SpeechEnded`, `Error`. The `language` field on `Final` carries an ISO-639-1 code when the provider reports detected language (Deepgram in multi mode; AssemblyAI does not currently report it).

`DisconnectResult` is `Option<(String, Option<String>)>` — file-based providers return `(text, detected_language)` on close. Streaming providers return `Ok(None)` and emit `Final` instead.

### Language hint mapping rule

`SttConfig.languages: Vec<String>` carries the user's selection (empty = auto-detect). Adapters map it to each provider's wire format:

- **Whisper-compatible** (`openai-whisper`, `groq-whisper`, `glm-asr`, `siliconflow`): the form field `language=<code>` is sent **only when `languages.len() == 1`**. Both 0 and >1 omit the field and let Whisper auto-detect — the Whisper API doesn't accept a set. All Whisper-compat requests also include `response_format=verbose_json` so the response carries the detected `language`.
- **Deepgram**: URL `?language=<code>` when `languages.len() == 1`; otherwise `?language=multi` (Deepgram's native multi-language mode handles both empty-set and many-set).
- **AssemblyAI**: the streaming WebSocket URL does not accept a language hint today; the field is silently ignored. Follow-up.

The multi-element selection in the UI is primarily a hint to the **polish prompt** (which receives the full `user_languages` set via `PolishRequest`) rather than the STT. The pipeline-level polish therefore biases toward the user's languages even when the wire-level STT request can't carry them.

### Verbose-json language extraction (Whisper-compat)

`response_format=verbose_json` returns a `language` value that providers spell either as ISO-639-1 (`"en"`) or as a lowercase English name (`"english"`). `whisper_compat::normalize_language` accepts both and silently falls back to `None` for unrecognized values — so a provider that omits the field, or returns a label outside our small table, simply produces a transcript with no detected-language badge rather than crashing. **Needs confirmation**: whether GLM-ASR and SiliconFlow honor `response_format=verbose_json` — both currently ignore unknown form fields, so requests succeed either way; only the badge is missing when they don't.

### Provider IDs in `create_provider`

Match arms currently registered in `stt::create_provider`:

- `assemblyai`
- `glm-asr`
- `openai-whisper`
- `groq-whisper`
- `siliconflow`
- `_` (default) → falls back to GLM-ASR.

`glm-asr`, `openai-whisper`, `groq-whisper`, and `siliconflow` share `WhisperCompatProvider` with different endpoints, models, and extra fields.

### Mismatches with the frontend list

`src/lib/constants.ts` and `src/stores/appStore.ts` also expose `deepgram` (label `Deepgram Nova-3`). Connection-test, benchmark, and pre-warm code in `src-tauri/src/lib.rs` and `src-tauri/src/pipeline.rs` recognise `deepgram`, but the streaming `create_provider` factory does not — selecting it currently falls through to the GLM-ASR default. **Needs confirmation**: whether this is an in-progress integration or a regression. The frontend list and the factory should match.

## LLM Providers

Trait in `src-tauri/src/llm/mod.rs`:

```rust
async fn polish(
    &self,
    config: &LlmConfig,
    req: &PolishRequest,
    on_chunk: Option<&ChunkCallback>,
) -> Result<PolishResponse>;

fn name(&self) -> &str;
```

`OpenAiProvider` handles OpenAI-compatible providers via `base_url` + `model`. All registered LLM providers go through it.

Frontend LLM provider IDs (`src/stores/appStore.ts`): `zhipu`, `deepseek`, `siliconflow`, `openai`, `gemini`, `moonshot`, `qwen`, `groq`, `claude`, `ollama`, `openrouter`.

## Adding A Provider

When adding a provider, update all of:

- The factory in `src-tauri/src/stt/mod.rs` or `src-tauri/src/llm/mod.rs`.
- The frontend IDs in `src/stores/appStore.ts` and labels in `src/lib/constants.ts`.
- Connection-test and benchmark match arms in `src-tauri/src/lib.rs`.
- Pre-warm endpoints in `src-tauri/src/pipeline.rs::pre_warm`.
- The Settings UI under `src/components/Settings/`.
- The relevant docs (this file and [Feature map](../domain/features.md)).

This list is the source of truth for the conventions checklist; [`references/conventions.md`](../references/conventions.md) defers to it.

## Needs confirmation

- No documented policy for when a new provider should be bespoke vs OpenAI-compatible vs Whisper-compatible.
- Provider reliability, latency, and quota expectations are not tracked in repo-local docs.
