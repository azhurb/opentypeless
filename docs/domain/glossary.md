# Glossary

## AppConfig

Rust and TypeScript config shape for user settings such as providers, API keys, hotkey, output mode, theme, and translation.

## BYOK

Bring Your Own Key. User configures provider API keys locally and requests go directly to the chosen provider.

## Capsule

Small transparent always-on-top window that shows recording, processing, and completion state.

## Cloud Provider

Special `cloud` STT or LLM provider that proxies through the OpenTypeless backend using a session bearer token.

## Dictionary

User-defined terms that are injected into LLM prompt building so exact spellings are preserved.

## Feature Map

Repo-local inventory of user-facing features reconciled against code evidence and public website claims.

## LLM

Large language model used to polish, format, or translate the raw transcript.

## Pipeline

Rust orchestration flow from recording through transcription, polishing, output, and history storage.

## Selected-Text Mode

Mode where selected foreground-app text is captured with Cmd/Ctrl+C and passed to the LLM. The voice input is treated as an instruction about that selected text.

## Scene Pack

Cloud-fetched pack containing a description, prompt template, and optional dictionary terms for a workflow.

## STT

Speech-to-text provider that converts microphone audio into transcript text.

## Tauri Command

Rust function exposed to the frontend through Tauri `invoke(...)`.

## TranscriptEvent

Rust event type from STT providers. Variants include partial transcript, final transcript, speech start/end, and errors.
