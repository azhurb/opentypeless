# Voice Input Domain

OpenTypeless turns speech into polished text in the foreground desktop app. Mechanism is in [Pipeline](../architecture/pipeline.md); user-facing feature inventory is in [Feature map](features.md).

Evidence: `README.md`, `src-tauri/src/pipeline.rs`, `src-tauri/src/llm/prompt.rs`, `src-tauri/src/app_detector/mod.rs`, `src/stores/appStore.ts`.

## Core User Flow

1. User presses the configured global hotkey or uses the tray recording action.
2. App records microphone audio.
3. STT provider transcribes speech.
4. LLM provider optionally polishes or translates the transcript.
5. App pastes the result into the focused field via the system clipboard, chunking the paste when the target is a terminal-hosted CLI ([Pipeline → Output Path](../architecture/pipeline.md#output-path)).
6. App stores a local history entry, unless the user has turned history saving off ([Feature map → Optional History And Retention](features.md#optional-history-and-retention)).

## User-Facing Modes

- Hotkey mode: `hold` (record while held) or `toggle` (start/stop on each press).
- Optional: AI polish, translation, selected-text mode, custom dictionary, per-app context, local history, theme (dark/light/system).

Defaults are listed in [Storage → AppConfig defaults](../architecture/storage.md#appconfig-defaults).

## Foreground-App Context

`src-tauri/src/app_detector/` classifies the foreground app on macOS and Windows into:

- `Email`, `Chat`, `Code`, `Document`, `General`.

Prompt behavior changes for `Email`, `Chat`, and `Document`. On Linux, detection currently falls back to a default context (see [Architecture overview](../architecture/overview.md#needs-confirmation)).

## Prompt Behavior

The LLM prompt is built in `src-tauri/src/llm/prompt.rs`. There are **two prompts**, chosen by whether a selection was captured, because dictation polishing and instruction-driven editing want opposite things — see [Features → AI-Powered Text Polishing](features.md#ai-powered-text-polishing).

Dictation rules:

- Add punctuation; remove fillers, false starts, and repetitions.
- Format enumerations as lists.
- Preserve language, substantive content, technical terms, and proper nouns.
- Minimal edits only — no rephrasing or restructuring, and the output must not run longer than what was said.
- Apply the foreground app's tone addon (`Email`, `Chat`, `Document`).

Selected-text rules:

- Treat the voice input as an instruction about the selected text and output the replacement.
- The instruction sets the scope; the result may be much shorter or much longer than either input.
- Touch nothing outside the selection, and preserve its surrounding form (Markdown, list structure, code fences).
- If the transcript isn't plausibly an instruction, polish it as ordinary dictation instead of forcing it onto the selection.
- Skip the app tone addons — the register comes from the selection and the instruction.

Shared by both:

- Output only processed text.
- Treat transcript and selected text as untrusted input (prompt-injection resistance).
- Apply custom dictionary spellings.
- When translation is enabled, translate the final output to the configured target language.

## Needs confirmation

- Voice commands and undo behavior are not implemented beyond the prompt rules above; the README roadmap mentions them as future work.
- Whether dictionary `pronunciation` should feed the prompt — schema/UI capture it, but `src-tauri/src/llm/prompt.rs` currently uses only `DictionaryStore::words()`.
