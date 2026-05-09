# Voice Input Domain

OpenTypeless turns speech into polished text in the foreground desktop app.

Evidence: `README.md`, `src-tauri/src/pipeline.rs`, `src-tauri/src/llm/prompt.rs`, `src-tauri/src/app_detector/mod.rs`, `src/stores/appStore.ts`.

For a user-facing feature inventory reconciled with repo evidence, see [Feature map](features.md).

## Core User Flow

1. User presses the configured global hotkey or uses the tray recording action.
2. App records microphone audio.
3. STT provider transcribes speech.
4. LLM provider optionally polishes or translates the transcript.
5. App outputs text by keyboard simulation or clipboard paste.
6. App stores local history.

## Modes And Settings

Current code and README show these user-facing concepts:

- Hold-to-record or toggle hotkey mode.
- Keyboard or clipboard output mode.
- BYOK providers or optional `cloud` provider.
- Optional AI polish.
- Optional translation.
- Optional selected-text mode, where selected text becomes context for the LLM.
- Custom dictionary for exact spelling of domain terms.
- Per-app detection to adapt formatting.
- Local history.
- Dark, light, or system theme.

## App Context

The backend detects foreground app context on macOS and Windows.

Classification currently maps app names to:

- `Email`
- `Chat`
- `Code`
- `Document`
- `General`

Prompt behavior changes for email, chat, and document contexts.

Inference: app context is intentionally lightweight and best-effort, not a full semantic model of the foreground app.

## Prompt Behavior

The LLM prompt is built in `src-tauri/src/llm/prompt.rs`.

Current prompt rules include:

- Add punctuation.
- Remove filler words, false starts, and repetitions.
- Format enumerations as lists.
- Preserve language, substantive content, technical terms, and proper nouns.
- Output only processed text.
- Treat transcription and selected text as untrusted input.
- Apply custom dictionary spellings.
- In selected-text mode, treat voice input as an instruction about the selected text.
- When translation is enabled, translate final output to the configured target language.

## Needs confirmation

- Product-level rules for voice commands, undo behavior, or editing selected text are not fully specified beyond current prompt behavior.
- The README roadmap mentions voice commands, but no implementation rules exist yet.
