# Glossary

## AppConfig

Rust + TypeScript config shape for user settings (providers, API keys, hotkey, theme, translation). Defaults: [Storage → AppConfig defaults](../architecture/storage.md#appconfig-defaults).

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

Mode where a dictation edits the user's selection instead of inserting text: the transcript becomes an instruction, the selection becomes the material, and the polished result replaces the selection. Captured Accessibility-first on macOS (`AXSelectedText`, read at record start), falling back to a Cmd/Ctrl+C copy where Accessibility is blind — browser web content, Electron, and every non-macOS platform. Requires AI Polish, since the LLM is what applies the instruction. Detail: [Pipeline → Selected-Text Capture](../architecture/pipeline.md#selected-text-capture).

## STT

Speech-to-text provider that converts microphone audio into transcript text. Provider catalogue: [Providers](../architecture/providers.md).

## Tauri Command

Rust function exposed to the frontend through Tauri `invoke(...)`. Registry rule: [Frontend ↔ Backend → Tauri Commands](../architecture/frontend-backend.md#tauri-commands).

## TranscriptEvent

Rust event type from STT providers. Variants: `Partial`, `Final`, `SpeechStarted`, `SpeechEnded`, `Error`.
