# Provider Retry + Pooled HTTP Client

Written 2026-07-26 as a handoff brief. Nothing implemented yet.

## Goal

A transient 429 or 502 from Deepgram / AssemblyAI / OpenRouter currently fails the whole
dictation: the user speaks for thirty seconds, waits, and gets an error for something that
would have succeeded on a second attempt. There is **no retry anywhere in `src-tauri/`**
(`grep -rn "retry\|backoff" src-tauri/src` finds only the unrelated AX snapshot retry in
`correction/`).

Ship retry with exponential backoff for the provider calls where it is safe, and pool the
HTTP client while in the same layer — retry without connection reuse pays a fresh TLS
handshake per attempt.

## What is safe to retry — read this before writing anything

`SttProvider` is **stateful and streaming** (`stt/mod.rs:60-65`): `connect`, `send_audio`,
`recv_transcript`, `disconnect`. Retry is *not* uniformly safe across it.

| Call | Retry? | Why |
| --- | --- | --- |
| `SttProvider::connect` | **Yes** | No session state yet; a failed connect is a clean slate |
| `SttProvider::send_audio` | **No** | Mid-stream resend reorders or duplicates audio |
| `SttProvider::recv_transcript` | **No** | Same — the session is stateful |
| `whisper_compat` file upload | **Yes** | One-shot multipart POST, idempotent |
| `LlmProvider::polish` | **Yes** | One-shot POST |
| Connection tests (`lib.rs` commands) | **No** | User-initiated; a failure is the answer they asked for |

A naive "wrap every provider call" would corrupt streaming sessions. Scope the helper to the
four **Yes** rows.

## Retryable error classes

Retry on: HTTP **429**, HTTP **5xx**, and `reqwest` connect/timeout errors.
Never retry on: 4xx other than 429 (auth, malformed request, quota-exhausted) — those do not
improve with another attempt, and retrying an auth failure just delays the error the user
needs to see.

Defaults: **3 attempts**, exponential backoff, **silent** to the user. The capsule already
shows a progress state during transcription; surfacing "retrying 2/3" adds noise to a path
whose goal is that nothing appears to have gone wrong. Revisit only if testing shows retries
routinely take long enough to look like a hang.

## Scope

Provider surface is small — roughly 1,100 lines total:

| File | Lines | Work |
| --- | --- | --- |
| `stt/mod.rs` | 112 | Trait stays as-is; wrap `connect` at the call site |
| `stt/deepgram.rs` | 204 | Retry `connect` |
| `stt/assemblyai.rs` | 143 | Retry `connect` |
| `stt/whisper_compat.rs` | 377 | Retry the upload; take the pooled client |
| `llm/openai.rs` | 204 | Retry `polish`; take the pooled client |
| `llm/mod.rs` | 81 | Trait unchanged |

**No `AppError` refactor needed.** Both traits already return `anyhow::Result`
(`stt/mod.rs:5`, `llm/mod.rs:4`), so the helper can classify on the `reqwest::Error` /
status directly. Upstream's version (`3689106` STT, `6855c54` LLM, helper in `7996aee`) is
entangled with their `AppError`/`UserError` types and their cloud providers — **read them for
the backoff shape, then write ours against `anyhow`**. Do not port the diff.

## Pooled client

12 `reqwest::Client::new()` sites today:

- `lib.rs:204,215,265,301,334,385,401,455,495` — nine, all in connection-test commands
- `pipeline.rs:217` — `shared_client`, already the pooled one
- `llm/openai.rs:21` and `stt/whisper_compat.rs:215` — construct their own, bypassing it

Put one `reqwest::Client` in Tauri state and hand it to the providers; `Client` is
`Arc`-internally so cloning is cheap. The two provider sites are the ones on the dictation
hot path and matter most. The nine command sites are user-initiated and low-frequency — fold
them in for consistency, not urgency.

## Fold in

`da7b5fd` (upstream, 22 lines in `commands/stt.rs`) — stop "Test connection" spending the
user's OpenAI Whisper quota. Same provider layer, trivially small, and a real BYOK annoyance:
we currently bill the user to verify their own key.

## Verification

CI runs on this fork now (fixed in #25), so a PR gets a real green check. Still run locally
first — see [commands.md](../../references/commands.md):

```bash
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

Add unit tests for the classifier — which statuses retry and which don't is exactly the part
that rots silently.

## Gotchas

- **Do not run a blanket `cargo fmt`.** The tree is rustfmt-clean as of #24; format only what
  you touch, or you will churn unrelated files.
- **Docs in the same PR.** CLAUDE.md rule 1 lists providers and pipeline behavior as triggers
  — expect to touch `docs/architecture/pipeline.md`.
- `typos-cli` is installed locally (`brew install typos-cli`); run `typos` rather than editing
  `.typos.toml` blind.
- Manual check worth doing: throttle or point a provider at a bad host and confirm a dictation
  survives a transient failure instead of dying.

## After this

The next item on the [upstream adoption](upstream-adoption.md) list is the **keychain
migration** — API keys move out of plaintext `settings.json` into the OS credential vault.
Highest-value remaining item; wants its own session.
