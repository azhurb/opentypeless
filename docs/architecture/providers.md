# Providers

STT and LLM integrations use trait plus factory patterns in Rust. Provider names are also represented in frontend TypeScript unions.

Evidence: `src-tauri/src/stt/mod.rs`, `src-tauri/src/llm/mod.rs`, `src/stores/appStore.ts`, `src/components/Settings/`.

## STT Providers

The `SttProvider` trait lives in `src-tauri/src/stt/mod.rs`.

```rust
async fn connect(&mut self, config: &SttConfig) -> Result<()>;
async fn send_audio(&mut self, chunk: &[u8]) -> Result<()>;
async fn recv_transcript(&mut self) -> Result<Option<TranscriptEvent>>;
async fn disconnect(&mut self) -> Result<Option<String>>;
fn name(&self) -> &str;
```

Current provider names visible in code:

- `cloud`
- `deepgram`
- `assemblyai`
- `glm-asr`
- `openai-whisper`
- `groq-whisper`
- `siliconflow`

`glm-asr`, `openai-whisper`, `groq-whisper`, and `siliconflow` share `WhisperCompatProvider` with different endpoints, models, and extra fields.

Streaming providers emit `TranscriptEvent` values: `Partial`, `Final`, `SpeechStarted`, `SpeechEnded`, and `Error`. File-based providers can return final transcript text from `disconnect()`.

## LLM Providers

The `LlmProvider` trait lives in `src-tauri/src/llm/mod.rs`.

```rust
async fn polish(
    &self,
    config: &LlmConfig,
    req: &PolishRequest,
    on_chunk: Option<&ChunkCallback>,
) -> Result<PolishResponse>;

fn name(&self) -> &str;
```

`OpenAiProvider` handles OpenAI-compatible providers through `base_url` and `model`. `CloudProvider` proxies through the OpenTypeless backend.

Current frontend LLM provider names visible in `src/stores/appStore.ts`:

- `zhipu`
- `deepseek`
- `siliconflow`
- `openai`
- `gemini`
- `moonshot`
- `qwen`
- `groq`
- `claude`
- `ollama`
- `openrouter`
- `cloud`

## Adding A Provider

When adding a provider, update:

- Rust provider factory in `src-tauri/src/stt/mod.rs` or `src-tauri/src/llm/mod.rs`.
- TypeScript provider union in `src/stores/appStore.ts`.
- Connection test and benchmark logic in `src-tauri/src/lib.rs`.
- Settings UI under `src/components/Settings/`.
- Any provider-specific defaults or labels.
- Relevant docs in this folder.

## Needs confirmation

- There is no documented policy for when a provider should be bespoke versus OpenAI-compatible or Whisper-compatible.
- Provider reliability, latency, and quota expectations are not documented in repo-local docs yet.
