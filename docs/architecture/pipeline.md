# Pipeline

`src-tauri/src/pipeline.rs` orchestrates recording → STT → LLM polish → keyboard/clipboard output. See [Providers](providers.md) for the provider traits used here and [Frontend ↔ Backend](frontend-backend.md) for the events the frontend listens to.

`PipelineHandle` is a Tauri-managed singleton. It holds: current `PipelineState`, audio handle, accumulated transcript, abort flag, preloaded config/context/dictionary, captured selected text, recording start time, shared `reqwest::Client`, and a `pipeline_lock`.

## States

`PipelineState`: `Idle` → `Recording` → `Transcribing` → `Polishing` → `Outputting` → `Idle`.

State changes emit `pipeline:state` to the frontend and update tray tooltip / capsule UI.

## Start Flow

1. `pipeline_lock` serializes setup so a fast press-release in hold mode cannot let `stop()` observe partially initialized state.
2. State moves `Idle → Recording`.
3. Config, current foreground-app context, and dictionary are loaded.
4. STT API config is built. An empty API key aborts the pipeline before audio capture starts.
5. STT provider connects before audio capture starts.
6. Audio capture starts and streams chunks to STT.
7. Partial and final transcript events are emitted to the frontend.

## Stop Flow

1. State moves `Recording → Transcribing`.
2. If selected-text mode is enabled, the pipeline waits `SELECTED_TEXT_CAPTURE_DELAY_MS` so hotkey modifiers can be released, then simulates Cmd/Ctrl+C and restores clipboard contents.
3. Audio capture stops; pipeline waits for STT finalization.
4. If polish is enabled, final text is sent to the LLM provider.
5. Output runs (clipboard paste — see [Output Path](#output-path)).
6. History is stored.
7. State returns to `Idle`.

## Events

Pipeline-related events emitted by the backend:

- `pipeline:state` — state transitions.
- `pipeline:error` — recoverable errors (STT/LLM/output failures, "no speech detected"). Two emitted payloads are matched on exactly by the frontend and trigger non-default UX: `ACCESSIBILITY_REQUIRED` (paste pre-flight saw no AX grant) and `MICROPHONE_DENIED` (record pre-flight saw `denied` / `restricted` mic status). Both are emitted bare, not wrapped in `"Output failed: …"`. See [Frontend ↔ Backend → Events](frontend-backend.md#events) for the frontend handling.
- `pipeline:target_app` — the foreground app captured for the current run.
- `audio:volume` — input level samples for the capsule waveform.
- `stt:partial`, `stt:final` — transcript updates.
- `llm:chunk` — streamed polished text from the LLM.
- `correction:suggest` — emitted to the capsule window when the post-dictation watcher finds a single-word substitution that passes the heuristic. Payload: `{ rowId, old, new, autoConfirmMs }`. The watcher runs only when `learn_from_corrections_enabled` is set in `AppConfig` and macOS Accessibility is granted.

This list is grep-verified from `src-tauri/src/pipeline.rs` and `src-tauri/src/lib.rs`. If an event is added or renamed, update [`frontend-backend.md`](frontend-backend.md) too.

## Output Path

Text is delivered exclusively via the system clipboard plus a synthesized Cmd+V (Ctrl+V on Windows/Linux). Implementation lives in `src-tauri/src/output/`:

- `clipboard.rs` snapshots the user's prior plain-text clipboard, writes the dictation text, sleeps `CLIPBOARD_SETTLE_MS` (30 ms), invokes paste via `enigo` (Cmd+V on macOS, Ctrl+V on Windows/Linux), then schedules a restore of the prior clipboard after `RESTORE_DELAY_MS` (500 ms). On macOS the Cmd+V key synthesis is dispatched onto Tauri's main thread via `AppHandle::run_on_main_thread` — `enigo`'s `CGEventSource::new` internally calls `TSMGetInputSourceProperty`, which the OS aborts the process for if invoked on a Tokio worker thread (modern macOS asserts main-thread for HIToolbox). The clipboard write itself stays on the worker thread (arboard is thread-safe).
- `chunker.rs` decides whether the paste should be split. For terminal-hosted CLIs that struggle with bulk pastes the text is broken into chunks separated by `INTER_CHUNK_DELAY_MS` (50 ms). Detection uses the foreground app's macOS bundle ID against a known terminal-like list (Terminal.app, iTerm2, Warp, Ghostty, Kitty, Alacritty, Hyper, WezTerm, VS Code, Cursor, Windsurf, JetBrains family) plus a case-insensitive substring match on the window title (`claude` → 800 chars / 2 newlines per chunk; `codex` → 1000 chars; `gemini` → 1000 chars). Non-terminal apps and shells with no recognised CLI fall through to a single bulk paste.
- macOS Cmd+V is synthesised by `enigo` via `CGEventPost`, which requires macOS **Accessibility** permission. The correction watcher uses Accessibility too, so this is a shared grant rather than a new one. There is no path to avoid Accessibility on modern macOS for keystroke synthesis; every alternative (`osascript "tell System Events to keystroke …"`, NSEvent simulation, AXUIElement post) ultimately routes through the same TCC check. `pipeline::output_text` pre-flights the grant with `is_accessibility_trusted()` and bails with `ACCESSIBILITY_REQUIRED` rather than letting the OS silently drop synthesised events when the grant is missing.

## Important Invariants

- `output_text()` trims trailing whitespace and appends a single space before pasting into the foreground app, so successive dictations don't glue together. History stores the un-normalized text.
- LLM polish output is batched: the capsule renders streamed `llm:chunk` events for a live indicator, but the paste only fires once polish completes.
- `pipeline_lock` serializes `start()` and `stop()`.
- `abort()` sets the abort flag, drops the audio handle, notifies `stt_done`, clears accumulated text, and forces `Idle`.
- On macOS, if Cmd+C does not change the clipboard, selected text is ignored — this avoids passing stale clipboard content to the LLM.
- macOS Accessibility permission is checked through raw FFI (`AXIsProcessTrusted`). It is required for output (keystroke synthesis of Cmd+V) and for the correction watcher (focused-field reads). A single grant covers both.
- Tray tooltip and capsule UI both subscribe to `pipeline:state`; consider both when changing state semantics.

## Needs confirmation

- User-facing retry semantics after STT or LLM failure are not documented separately from current code behavior.
- `AppConfig.max_recording_seconds` (default 30) is enforced in code; the precise enforcement path should be documented after a focused code review.
