# Glossary

## AppConfig

Rust + TypeScript config shape for user settings (providers, API keys, hotkey, output mode, theme, translation). Defaults: [Storage → AppConfig defaults](../architecture/storage.md#appconfig-defaults).

## BYOK

Bring Your Own Key. The user configures provider API keys locally and requests go directly to the chosen provider. This fork is BYOK-only — there are no cloud or proxy modes.

## Capsule

Small transparent always-on-top window that shows recording, processing, and completion state. See [Frontend ↔ Backend → Two Windows](../architecture/frontend-backend.md#two-windows-one-bundle).

## Dictionary

User-defined terms injected into the LLM prompt so exact spellings are preserved.

## Feature Map

Repo-local inventory of user-facing features reconciled against code evidence and public website claims: [`features.md`](features.md).

## LLM

Large language model used to polish, format, or translate the raw transcript.

## Pipeline

Rust orchestration flow from recording through transcription, polishing, output, and history storage. Detail: [Pipeline](../architecture/pipeline.md).

## Selected-Text Mode

Mode where selected foreground-app text is captured with Cmd/Ctrl+C and passed to the LLM. Voice input becomes an instruction about that selected text.

## STT

Speech-to-text provider that converts microphone audio into transcript text. Provider catalogue: [Providers](../architecture/providers.md).

## Tauri Command

Rust function exposed to the frontend through Tauri `invoke(...)`. Registry rule: [Frontend ↔ Backend → Tauri Commands](../architecture/frontend-backend.md#tauri-commands).

## TranscriptEvent

Rust event type from STT providers. Variants: `Partial`, `Final`, `SpeechStarted`, `SpeechEnded`, `Error`.
