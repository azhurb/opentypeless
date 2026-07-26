# Providers

STT and LLM integrations use trait + factory patterns in Rust. Provider IDs also appear in the frontend Zustand store. The IDs in `appStore.ts` and the match arms in the Rust factories must stay in sync.

Used by: [Pipeline](pipeline.md) (calls `create_provider`), [Feature map](../domain/features.md) (user-facing labels).

Evidence: `src-tauri/src/stt/mod.rs`, `src-tauri/src/llm/mod.rs`, `src-tauri/src/retry.rs`, `src/stores/appStore.ts`, `src/lib/constants.ts`, `src/components/Settings/`.

## Pooled HTTP Client

There is exactly one `reqwest::Client` in the app: `crate::HttpClient`, built in `lib.rs::setup` and Tauri-managed. `Client` is `Arc`-backed, so clones share its connection pool and warm TLS sessions.

Both factories take that client by value — `create_provider(provider_name, client)` — rather than an `Option`, so no provider can build its own and silently opt out of the pool. `PipelineHandle` holds a clone (`shared_client`) and hands one to each provider it constructs; the connection-test and benchmark commands in `lib.rs` read it from managed state. Connection reuse matters more now that calls retry: without it every attempt pays a fresh handshake.

## Retry Policy

Provider calls retry transient failures with exponential backoff. The helper lives in `src-tauri/src/retry.rs`; the policy is 3 attempts total, 400 ms backoff doubling to 800 ms, plus a **10 s time budget**: once failed attempts have consumed that much wall-clock, the error surfaces instead of being retried.

The time budget is what makes attempt-counting safe. Provider requests carry a 60 s `reqwest` timeout and a timeout is a transient error, so a count-only policy could sit for three minutes — past `pipeline::STT_FINALIZE_TIMEOUT_SECS` (120 s), which would abandon the transcript, and `LlmProvider::polish` has no outer deadline at all. Retry is worth it while failures are cheap (a 429 or 502 answers in milliseconds, a refused connection immediately); a slow failure means the provider is in trouble and the user is better served by the error. In practice a fast-failing call adds at most 1.2 s.

Retry is **not** uniformly safe across the provider surface, because `SttProvider` is a stateful streaming session:

| Call | Retries | Why |
| --- | --- | --- |
| `SttProvider::connect` (Deepgram, AssemblyAI) | Yes | No session state exists yet; a failed handshake is a clean slate |
| `SttProvider::send_audio` | No | A mid-stream resend reorders or duplicates audio |
| `SttProvider::recv_transcript` | No | Same — the session is stateful |
| Whisper-compatible file upload (in `disconnect`) | Yes | One idempotent multipart POST, and nothing is on screen yet |
| `LlmProvider::polish` | Request head only | Retries `send()` plus the status check; stops once chunks reach the callback, since re-running would duplicate text the user can already see |
| Connection tests / benchmarks (`lib.rs`) | No | User-initiated; a failure is the answer they asked for |

**What counts as transient:** HTTP 429 and 5xx, `reqwest` connect/timeout errors, and WebSocket handshake failures at the socket layer (`tungstenite::Error::Io`) or with a retryable HTTP status. Everything else is fatal on the first attempt — a bad key, a malformed request or an exhausted quota returns the same answer forever, and retrying it only delays the error the user needs to see. An error the classifier does not recognize is treated as fatal.

Classification survives the `anyhow` boundary via `retry::HttpStatusError`, which carries the status while displaying the provider's own message. This is the part that rots silently: replacing it with a plain `anyhow::bail!` erases the status and turns every transient 503 back into a lost dictation, so both call sites (`stt::whisper_compat::upload_error`, `llm::openai::api_error`) are unit-tested for it.

**Retries are silent.** The capsule already shows a progress state for the step being retried, and a "retrying 2/3" surface would turn a recovery nobody was meant to notice into an apparent fault. Attempts are logged at warn level instead. Revisit only if retries turn out to take long enough to read as a hang.

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

- `deepgram`
- `assemblyai`
- `glm-asr`
- `openai-whisper`
- `groq-whisper`
- `siliconflow`
- `_` (default) → falls back to GLM-ASR.

`glm-asr`, `openai-whisper`, `groq-whisper`, and `siliconflow` share `WhisperCompatProvider` with different endpoints, models, and extra fields.

### Connection tests and benchmarks

`test_stt_connection` and `bench_stt_connection` in `lib.rs` probe a key without running a dictation. The key comes either from the command's `api_key: Option<String>` (a candidate the user typed but has not saved) or, when that is `None`, from the credential vault — see [Storage → Credentials](storage.md#credentials-os-credential-vault). Probing never persists the candidate. Deepgram and AssemblyAI use a cheap authenticated `GET`. The Whisper-compatible providers upload a 0.1 s silent WAV, sharing endpoint/model/extra-field resolution through `whisper_compat_test_target`.

`openai-whisper` is the exception: OpenAI bills every `/audio/transcriptions` call, so the upload probe charged the user to verify their own key — a real annoyance in a BYOK app. It now reads `GET /v1/models/whisper-1` instead, which proves the key is accepted for free. The other Whisper-compatible providers keep the upload probe. One consequence worth knowing: the benchmark number shown for `openai-whisper` is now a model-read round-trip rather than a transcription round-trip, so it is not comparable with the other Whisper-compatible providers' figures. **Needs confirmation**: whether GLM-ASR, Groq and SiliconFlow expose an equivalent per-model endpoint, and whether their transcription calls are billed the same way — if both hold, they should move to the same probe.

### Draining the close of a streaming session

`disconnect()` on a streaming provider sends the provider's finish signal and then **reads what comes back** before closing the socket, returning any flushed text through the existing `DisconnectResult` channel (the same one file-based providers use). `stt::drain_final_text` implements the loop; each provider passes its own message parser.

This exists because both providers used to send the signal and shut the socket in the same breath, dropping whatever the server sent in response: for Deepgram the results still pending at `CloseStream`, for AssemblyAI the formatted version of the turn in progress. Since the pipeline accumulates from `Final` events only, that cost the tail of an utterance — and all of it for a dictation short enough to be a single turn.

The drain is bounded twice, because it sits between the user releasing the hotkey and text appearing: a `DRAIN_IDLE_MS` (150 ms) gap with nothing received ends it, and `DRAIN_TOTAL_MS` (600 ms, checked between reads) caps it even if the provider keeps sending. A provider that answers promptly costs only its own flush time. `TranscriptEvent::SpeechEnded` is the stop signal — AssemblyAI's `Termination` maps to it, meaning the server has nothing left; it is the one place that variant is load-bearing.

**Needs confirmation** — the timing constants and the duplication question both want one live dictation per provider to settle: whether 150 ms is long enough for the flush to arrive, and whether AssemblyAI can re-send a formatted turn that was already delivered as a `Final` during the session (per protocol it finalizes only the turn in progress, so the drain does not de-duplicate). The symptom to watch for is a duplicated last sentence rather than a missing one.

### Deepgram result parsing

`deepgram::parse_result_message` turns one `Results` message into a `TranscriptEvent`, kept pure so the protocol handling is unit-tested without a socket. Empty transcripts (keep-alives, metadata, silent segments) yield `None`; `is_final: false` yields `Partial`; a finalized segment yields `Final` with confidence and `channel.detected_language`.

A finalized segment yields its text **even when `speech_final` is set**. Deepgram marks end-of-speech on the message that also carries the last words of the utterance, so treating `speech_final` as a pure signal drops them — and nothing downstream notices, because the pipeline ignores `TranscriptEvent::SpeechEnded` (`pipeline.rs`, `_ => {}`) and drives finalization from the audio channel closing. That was live in the provider until the factory arm was wired up; there is a regression test on it.

**Resolved** (was a `Needs confirmation` on the frontend/factory mismatch): `src/lib/constants.ts` and `src/stores/appStore.ts` expose `deepgram` (label `Deepgram Nova-3`), and the connection-test, benchmark and pre-warm paths in `lib.rs` / `pipeline.rs` have always recognised it — but `stt::create_provider` had no arm for it since the initial commit, so selecting Deepgram silently fell through to the GLM-ASR default and authenticated a GLM-ASR request with a Deepgram key. The arm now exists. `DeepgramProvider` was dead code for that whole period, so its end-to-end behavior is **unverified against the live API** — the parsing is unit-tested, but nothing here has been exercised with a real Deepgram key.

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
