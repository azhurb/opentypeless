# Pipeline

`src-tauri/src/pipeline.rs` orchestrates recording → STT → LLM polish → keyboard/clipboard output. See [Providers](providers.md) for the provider traits used here and [Frontend ↔ Backend](frontend-backend.md) for the events the frontend listens to.

`PipelineHandle` is a Tauri-managed singleton. It holds: current `PipelineState`, audio handle, accumulated transcript, abort flag, preloaded config/context/dictionary, captured selected text, recording start time, shared `reqwest::Client`, and a `pipeline_lock`.

## States

`PipelineState`: `Idle` → `Recording` → `Transcribing` → `Polishing` → `Outputting` → `Idle`.

State changes emit `pipeline:state` to the frontend and update tray tooltip / capsule UI.

## Start Flow

1. `pipeline_lock` serializes setup so a fast press-release in hold mode cannot let `stop()` observe partially initialized state.
2. State moves `Idle → Recording`.
3. Audio capture opens first, before any of the slow async setup. The cpal stream feeds an mpsc channel bounded at ~4 s of headroom (200 chunks of 20 ms), so samples buffer locally while the rest of setup runs — collapsing the dead window between hotkey press and first-captured audio.
4. Config, current foreground-app context, and dictionary are loaded.
5. STT API config is built. An empty API key aborts the pipeline, tearing down the running audio capture via `cleanup_failed_start()`.
6. STT provider connects. For streaming providers (Deepgram, AssemblyAI) this is a full WebSocket handshake — audio keeps buffering during the handshake.
7. The STT forwarder task spawns, immediately flushing any pre-buffered chunks into the now-connected provider.
8. Partial and final transcript events are emitted to the frontend.

Background: audio capture used to be the *last* step of setup. With streaming STT the WebSocket handshake (~100–500 ms) plus foreground-app detection plus cpal cold-start meant the first ~300–1200 ms of user speech was discarded. See [`docs/plans/active/dictation-startup-latency.md`](../plans/active/dictation-startup-latency.md) for the full timing breakdown, the macOS native-detection rewrite that lives alongside this change, and the deferred follow-ups.

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
- `pipeline:timing` — per-dictation summary fired after output completes. Payload: `{ stt_ms, llm_ms, total_ms, recording_ms, detected_language }`. `detected_language` is the ISO-639-1 code reported by the STT for this utterance (`null` when unavailable). The frontend `useDetectedLanguageNotifier` hook uses this to fire a rate-limited toast when the detected language isn't in `config.stt_languages`.
- `correction:suggest` — emitted to the capsule window when the post-dictation watcher finds a single-word substitution that passes the heuristic. Payload: `{ rowId, old, new, autoConfirmMs }`. The watcher runs only when `learn_from_corrections_enabled` is set in `AppConfig` and macOS Accessibility is granted.

### Detected language threading

When the STT reports a language (Whisper-compatible providers via `response_format=verbose_json`, Deepgram via `channel.detected_language` in multi mode), `PipelineHandle.detected_language: Arc<Mutex<Option<String>>>` captures it from either the streaming `TranscriptEvent::Final.language` or the file-based `disconnect()` tuple. The chokepoint at `stop()` reads this and passes it into:

1. `PolishRequest.detected_language` — the LLM polish prompt receives a one-line context hint (rendered as a display name, never raw text from the wire).
2. `PolishRequest.user_languages` — the polish prompt also receives the user's configured set so it can disambiguate when detection is wrong.
3. `HistoryEntry.language` — persisted to SQLite so the History view can render a per-row badge.
4. `pipeline:timing.detected_language` — emitted to the frontend for the wrong-language toast.

This list is grep-verified from `src-tauri/src/pipeline.rs` and `src-tauri/src/lib.rs`. If an event is added or renamed, update [`frontend-backend.md`](frontend-backend.md) too.

## Output Path

Text is delivered exclusively via the system clipboard plus a synthesized Cmd+V (Ctrl+V on Windows/Linux). Implementation lives in `src-tauri/src/output/`:

- `clipboard.rs` snapshots the user's prior plain-text clipboard, writes the dictation text, sleeps `CLIPBOARD_SETTLE_MS` (30 ms), invokes paste, then schedules a restore of the prior clipboard after `RESTORE_DELAY_MS` (500 ms).
  - **macOS paste** is synthesised directly via `core-graphics`. Two CGEvents (V key-down, V key-up) are built from an `HIDSystemState` event source, `kCGEventFlagMaskCommand` is stamped on each event with `CGEventSetFlags`, and the events are posted to `kCGHIDEventTap` with a 5 ms gap between down and up. No separate Cmd `flagsChanged` events are posted — the modifier travels on the V event itself, which is the canonical pattern for synthesising shortcut keystrokes on modern macOS. Synthesis is marshalled onto Tauri's main thread via `AppHandle::run_on_main_thread`; the clipboard write stays on the worker thread (arboard is thread-safe). Background: `enigo` 0.2.x posts the modifier as a separate `flagsChanged` event and relies on `CombinedSessionState` to propagate the flag onto the next-created V event; under load the V event is created before the flag has propagated, the receiving app (notably Chromium/Electron text inputs) sees a plain V keystroke, and the user gets a literal "v" typed instead of paste. Building the V event with the flag pre-set sidesteps the race.
  - **Windows / Linux paste** is synthesised via `enigo` (Ctrl press → V click → Ctrl release). The macOS-only race does not occur on these platforms.
- `chunker.rs` decides whether the paste should be split. For terminal-hosted CLIs that struggle with bulk pastes the text is broken into chunks separated by `INTER_CHUNK_DELAY_MS` (50 ms). Detection uses the foreground app's macOS bundle ID against a known terminal-like list (Terminal.app, iTerm2, Warp, Ghostty, Kitty, Alacritty, Hyper, WezTerm, VS Code, Cursor, Windsurf, JetBrains family) plus a case-insensitive substring match on the window title (`claude` → 800 chars / 2 newlines per chunk; `codex` → 1000 chars; `gemini` → 1000 chars). Non-terminal apps and shells with no recognised CLI fall through to a single bulk paste.
- macOS Cmd+V via `CGEventPost` requires macOS **Accessibility** permission. The correction watcher uses Accessibility too, so this is a shared grant rather than a new one. There is no path to avoid Accessibility on modern macOS for keystroke synthesis; every alternative (`osascript "tell System Events to keystroke …"`, NSEvent simulation, AXUIElement post) ultimately routes through the same TCC check. `pipeline::output_text` pre-flights the grant with `is_accessibility_trusted()` and bails with `ACCESSIBILITY_REQUIRED` rather than letting the OS silently drop synthesised events when the grant is missing.

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
