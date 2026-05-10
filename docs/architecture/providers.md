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
async fn disconnect(&mut self) -> Result<Option<String>>;
fn name(&self) -> &str;
```

`TranscriptEvent` variants: `Partial`, `Final`, `SpeechStarted`, `SpeechEnded`, `Error`. File-based providers can also return final text from `disconnect()`.

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
