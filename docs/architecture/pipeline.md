# Pipeline

`src-tauri/src/pipeline.rs` orchestrates recording → STT → LLM polish → keyboard/clipboard output. See [Providers](providers.md) for the provider traits used here and [Frontend ↔ Backend](frontend-backend.md) for the events the frontend listens to.

`PipelineHandle` is a Tauri-managed singleton. It holds: current `PipelineState`, audio handle, accumulated transcript, abort flag, preloaded config/context/dictionary, captured selected text, recording start time, a clone of the app-wide pooled `reqwest::Client` (`crate::HttpClient`, passed to `PipelineHandle::new` and handed to every provider — see [Providers → Pooled HTTP Client](providers.md#pooled-http-client)), and a `pipeline_lock`.

## States

`PipelineState`: `Idle` → `Recording` → `Transcribing` → `Polishing` → `Outputting` → `Idle`.

State changes emit `pipeline:state` to the frontend and update tray tooltip / capsule UI.

## Start Flow

1. `pipeline_lock` serializes setup so a fast press-release in hold mode cannot let `stop()` observe partially initialized state.
2. State moves `Idle → Recording`.
3. Audio capture opens first, before any of the slow async setup. The cpal stream feeds an mpsc channel bounded at ~4 s of headroom (`CHANNEL_CHUNK_CAPACITY`, 200 chunks of 20 ms), so samples buffer locally while the rest of setup runs — collapsing the dead window between hotkey press and first-captured audio.

   **That headroom is a deadline, not a cushion.** Nothing drains the channel until the forwarder task spawns in step 8, so any step between here and there that takes longer than the remaining headroom costs the user the start of their sentence. Chunks past the limit are dropped and unrecoverable. The realtime cpal callback counts them in an atomic rather than logging — `tracing` there would allocate and take locks on the one thread that must never block — and `run_capture` warns with the total and its millisecond equivalent when capture stops. Silence in the log means nothing was lost; before 0.7.1 it meant nothing was *reported*, so a truncated transcript looked like a user who hadn't spoken.
4. Config, current foreground-app context, and dictionary are loaded.
5. **Selected-text Accessibility preflight** (macOS). When `should_capture_selection` passes, `correction::focused_selected_text()` reads `AXSelectedText` off the system-wide focused element. A hit is stored in `preloaded_selected_text`, and is the **only** way a run enters edit mode. Either way `pipeline:editing_selection` is emitted with the boolean — this is the **only** place it is emitted. Placed here — after app detection, before the STT connect — so the capsule can show the editing indicator while the user is still speaking. Being a read-only AX query rather than a keystroke is what makes it safe with the hotkey still physically held. See [Selected-Text Capture](#selected-text-capture).

   The read is blocking FFI, so it runs on `spawn_blocking` rather than on a runtime worker, and the wait is bounded twice: `SELECTION_PREFLIGHT_TIMEOUT_MS` (500 ms) caps how long `start()` waits, and `AX_MESSAGING_TIMEOUT_SECS` (0.4 s, set on the system-wide element and therefore process-wide — the correction watcher reads the same way and should not hang either) caps each individual AX message. Both exist because of step 3's deadline: an unresponsive foreground app would otherwise stall setup while audio fell off the end of the channel. Losing the preflight means this dictation is inserted rather than treated as an edit — there is no second chance later in the run.
6. STT API config is built. The key is read from the OS credential vault by `(stt, provider)` — it is not in `AppConfig`. No key aborts the pipeline, tearing down the running audio capture via `cleanup_failed_start()`; a vault that cannot be *read* aborts with a distinct message, since telling the user to re-enter a key that is already there sends them the wrong way. Only the key's length is ever logged.
7. STT provider connects. For streaming providers (Deepgram, AssemblyAI) this is a full WebSocket handshake — audio keeps buffering during the handshake, including across a retried attempt (see [Transient Failure Retry](#transient-failure-retry)).
8. The STT forwarder task spawns, immediately flushing any pre-buffered chunks into the now-connected provider.
9. Partial and final transcript events are emitted to the frontend.

Background: audio capture used to be the *last* step of setup. With streaming STT the WebSocket handshake (~100–500 ms) plus foreground-app detection plus cpal cold-start meant the first ~300–1200 ms of user speech was discarded. See [`docs/plans/active/dictation-startup-latency.md`](../plans/active/dictation-startup-latency.md) for the full timing breakdown, the macOS native-detection rewrite that lives alongside this change, and the deferred follow-ups.

## Stop Flow

1. State moves `Recording → Transcribing`.
2. Audio capture stops; pipeline waits for STT finalization. Closing the audio channel is what drives `disconnect()`, and for streaming providers that call now briefly drains whatever the provider flushes in response to its finish signal — see [Providers → Draining the close of a streaming session](providers.md#draining-the-close-of-a-streaming-session). Text recovered there arrives through the same `DisconnectResult` branch that file-based providers use.
3. If polish is enabled, final text is sent to the LLM provider. A transient failure of the request itself is retried (see [Transient Failure Retry](#transient-failure-retry)).
4. Output runs (clipboard paste — see [Output Path](#output-path)).
5. History is stored — **only if `history_enabled`**, re-read here rather than taken from the recording-start config snapshot so a mid-dictation opt-out is honored. When it is off the insert is skipped and only the retention prune runs. See [Storage → Retention](storage.md#retention).
6. State returns to `Idle`.

## Selected-Text Capture

When `selected_text_enabled` is on, a dictation *edits the user's selection* instead of inserting text: the transcript becomes an instruction, the selection becomes the material, and the polished result replaces the selection. There is exactly one way in.

**Gate.** `should_capture_selection(selected_text_enabled, polish_enabled)` — both must be true. Only the LLM request ever reads the selection, so with polish off looking for one would cost latency to produce something nothing consumes. The Settings toggle is disabled in the same state, so this is the second of two layers.

**Accessibility preflight (macOS, start step 5).** `correction::focused_selected_text()` reads `AXSelectedText` off the system-wide focused element, guarding against `AXSecureTextField` so password fields are never read. Read-only — no keystroke, no clipboard write — so it is safe while the hotkey is still physically held, which is what lets the capsule show the editing indicator during recording rather than after release.

**Why there is no clipboard fallback any more.** Through 0.7.1 a `Cmd`/`Ctrl+C` capture ran in `stop()` whenever the preflight came up empty, on the reasoning that a `None` from Accessibility cannot distinguish "nothing is selected" from "Accessibility is blind here". That reasoning was sound; the implementation could not act on it. The fallback classified *any* clipboard change as "the user selected text to edit", and a copy keystroke does not mean what that assumes:

- VS Code and its forks default `editor.emptySelectionClipboard` to `true`, so `Cmd+C` with **no selection at all** copies the whole current line. JetBrains IDEs behave the same way. These are Electron apps, invisible to Accessibility, so they landed in the fallback on every dictation.
- The result was scored as a selection, which switched the request to `SELECTED_TEXT_PROMPT` — a prompt that explicitly permits rewriting, reordering and changing tone, the exact opposite of the dictation prompt's "minimal edits, do not rephrase".
- Nothing on screen said so. The mode ring is deliberately suppressed for a selection discovered after the user has already spoken (see below), so an ordinary dictation could come back as a rewrite of a line the user never selected, with no indication that a different mode had engaged.

Narrowing the heuristic was considered and rejected: sentinel-clearing the clipboard detects "the copy did nothing" but not "the app copied something else", which is the actual failure. Losing an edit is recoverable; silently rewriting text the user did not select is not. So edit mode now requires the positive Accessibility signal, and nothing else.

**What that costs.** Voice-editing no longer reaches browser web content or Electron apps (Cursor, VS Code, Slack), and — since `correction::ax_stub` has no implementation — it does not work off macOS at all. The Settings toggle is disabled on Windows and Linux for the same reason it is disabled when polish is off: a switch that can never fire is worse than an absent one. Ways to win those targets back are in [`docs/plans/active/selected-text-in-ax-blind-apps.md`](../plans/active/selected-text-in-ax-blind-apps.md).

**The mode ring now means exactly one thing.** `pipeline:editing_selection` fires once per run from `start()` only: the preflight saw a selection before you spoke. Because that is now also the only path into edit mode, the ring is no longer a partial signal — no ring means the dictation will be inserted, every time. This is what the ring was always supposed to promise; the fallback is what made it a half-truth.

`replaced_selection` — whether the LLM request carried a selection — then drives three things downstream: no trailing space on output, no raw-transcript fallback if the LLM call fails, and no correction watcher. See [Important Invariants](#important-invariants).

## Transient Failure Retry

A 429 or 5xx from the STT or LLM provider used to fail the whole dictation: the user spoke for thirty seconds, waited, and got an error for a call that would have succeeded on a second attempt. Three points in the flow now retry with exponential backoff (3 attempts, 400 ms doubling to 800 ms) — the streaming STT handshake in start step 7, the Whisper-compatible upload that produces the transcript on `disconnect()`, and the request head of the LLM polish in stop step 3.

Retry is deliberately **not** applied to mid-stream calls (`send_audio`, `recv_transcript`), because the STT session is stateful and a resend would reorder or duplicate audio, nor to the LLM response body once chunks have started reaching `llm:chunk`. The full safety table, what counts as transient, and why retries emit no user-facing event live in [Providers → Retry Policy](providers.md#retry-policy).

No new pipeline state or event is involved: from the frontend's point of view a retried dictation is one that simply took a little longer — at most 1.2 s longer for the fast failures retry is meant for.

The interaction to keep in mind when touching either number: retries are also bounded by a 10 s time budget precisely so they cannot outlive `STT_FINALIZE_TIMEOUT_SECS` (120 s). The Whisper-compatible upload runs inside the STT forwarder task, and if it were still retrying when that deadline fired, `stop()` would give up waiting and proceed with the accumulated text — which for a file-based provider is empty, turning a recoverable blip into a lost dictation reported as "no speech".

## Events

Pipeline-related events emitted by the backend:

- `pipeline:state` — state transitions.
- `pipeline:error` — recoverable errors (STT/LLM/output failures, "no speech detected"). Two emitted payloads are matched on exactly by the frontend and trigger non-default UX: `ACCESSIBILITY_REQUIRED` (paste pre-flight saw no AX grant) and `MICROPHONE_DENIED` (record pre-flight saw `denied` / `restricted` mic status). Both are emitted bare, not wrapped in `"Output failed: …"`. See [Frontend ↔ Backend → Events](frontend-backend.md#events) for the frontend handling.

  **Provider failures are worded, never echoed.** `pipeline::provider_error_message` turns a failed provider call into `"<stage>: <reason>"` — stage being `Speech`, `Polish` or `Edit`, reason coming from [`retry::classify`](providers.md#retry-policy) (`daily quota reached`, `rate limited`, `API key rejected`, `out of credit`, `model not found`, `request too large`, `provider unavailable`, `provider timed out`, `cannot reach provider`, `provider error`). The provider's own body never reaches the capsule: it is JSON, it can carry the account's organization ID, and the error pill is one truncated line about 29 characters wide, so echoing it showed the user a prefix and nothing else. The full error goes to the log at error level instead. A unit test holds every message to the 29-character budget — a message the user cannot finish reading is the defect this replaced.

  **The empty-transcript branch defers to the STT error.** `stop()` reports `"No speech detected"` only when transcription itself succeeded. Any STT failure is recorded in `PipelineHandle::stt_error` as it happens (streaming error frame, `disconnect()` failure, or a dropped `recv_transcript`) and re-emitted from that branch, because the last `pipeline:error` payload wins in the frontend store: until 0.7.1 the specific error was emitted first and then overwritten here, so a rate-limited provider presented itself as a microphone problem. The slot is cleared wherever `accumulated_text` is.
- `pipeline:target_app` — the foreground app captured for the current run.
- `pipeline:editing_selection` — boolean payload: whether the Accessibility preflight found a selection *before the user spoke*. Emitted exactly once per run, from `start()` only, `false` included — so a dictation with nothing selected positively clears the flag the previous run set instead of relying on the idle transition having fired. The capsule maps it to an amber mode ring. Since the preflight is also the only way into edit mode, the ring is a total signal rather than a partial one: no ring means the dictation is inserted. The clipboard fallback that used to enter edit mode without emitting this is gone. See [Selected-Text Capture](#selected-text-capture).
- `audio:volume` — input level samples for the capsule waveform.
- `stt:partial`, `stt:final` — transcript updates.
- `llm:chunk` — streamed polished text from the LLM.
- `pipeline:timing` — per-dictation summary fired after output completes. Payload: `{ stt_ms, llm_ms, total_ms, recording_ms, detected_language }`. `detected_language` is the ISO-639-1 code reported by the STT for this utterance (`null` when unavailable). The frontend `useDetectedLanguageNotifier` hook uses this to fire a rate-limited toast when the detected language isn't in `config.stt_languages`.
- `correction:suggest` — emitted to the capsule window when the post-dictation watcher finds a single-word substitution that passes the heuristic. Payload: `{ rowId, old, new, autoConfirmMs }`. The watcher runs only when `learn_from_corrections_enabled` is set in `AppConfig` and macOS Accessibility is granted.
- `output:no_target` — emitted to the capsule window (no payload) when a paste did not land anywhere (nothing consumed the clipboard) and the dictation was left on the clipboard for a manual paste. The capsule shows a "press ⌘V to paste" tip. macOS only; never fired for terminals or chunked pastes. See [Output Path → Paste-landing detection](#paste-landing-detection).
- `output:edited` — emitted to the capsule window (no payload) when a paste **landed** and it replaced a selection. The capsule shows an "Edited — press ⌘Z to undo" tip: this is the one output path that destroys something the user already had, so it gets an explicit receipt carrying the undo shortcut. Mutually exclusive with `output:no_target` by construction — a paste sitting unclaimed on the clipboard has nothing to confirm.

### Detected language threading

When the STT reports a language (Whisper-compatible providers via `response_format=verbose_json`, Deepgram via `channel.detected_language` in multi mode), `PipelineHandle.detected_language: Arc<Mutex<Option<String>>>` captures it from either the streaming `TranscriptEvent::Final.language` or the file-based `disconnect()` tuple. The chokepoint at `stop()` reads this and passes it into:

1. `PolishRequest.detected_language` — the LLM polish prompt receives a one-line context hint (rendered as a display name, never raw text from the wire).
2. `PolishRequest.user_languages` — the polish prompt also receives the user's configured set so it can disambiguate when detection is wrong.
3. `HistoryEntry.language` — persisted to SQLite so the History view can render a per-row badge.
4. `pipeline:timing.detected_language` — emitted to the frontend for the wrong-language toast.

This list is grep-verified from `src-tauri/src/pipeline.rs` and `src-tauri/src/lib.rs`. If an event is added or renamed, update [`frontend-backend.md`](frontend-backend.md) too.

## Output Path

Text is delivered exclusively via the system clipboard plus a synthesized Cmd+V (Ctrl+V on Windows/Linux). Implementation lives in `src-tauri/src/output/`:

- `clipboard.rs` snapshots the user's prior plain-text clipboard, writes the dictation text, sleeps `CLIPBOARD_SETTLE_MS` (30 ms), invokes paste, then restores the prior clipboard (after `RESTORE_DELAY_MS`, 500 ms, on the eager path). The single, non-terminal paste path instead uses **paste-landing detection** (below) to decide whether to restore. `paste()` returns a `PasteOutcome { landed }` the pipeline uses to drive the no-target tip.
  - **macOS paste** is synthesised directly via `core-graphics`. Two CGEvents (V key-down, V key-up) are built from an `HIDSystemState` event source, `kCGEventFlagMaskCommand` is stamped on each event with `CGEventSetFlags`, and the events are posted to `kCGHIDEventTap` with a 5 ms gap between down and up. No separate Cmd `flagsChanged` events are posted — the modifier travels on the V event itself, which is the canonical pattern for synthesising shortcut keystrokes on modern macOS. Synthesis is marshalled onto Tauri's main thread via `AppHandle::run_on_main_thread`; the clipboard write stays on the worker thread (arboard is thread-safe). Background: `enigo` 0.2.x posts the modifier as a separate `flagsChanged` event and relies on `CombinedSessionState` to propagate the flag onto the next-created V event; under load the V event is created before the flag has propagated, the receiving app (notably Chromium/Electron text inputs) sees a plain V keystroke, and the user gets a literal "v" typed instead of paste. Building the V event with the flag pre-set sidesteps the race.
  - **Windows / Linux paste** is synthesised via `enigo` (Ctrl press → V click → Ctrl release). The macOS-only race does not occur on these platforms.
- `chunker.rs` decides whether the paste should be split. For terminal-hosted CLIs that struggle with bulk pastes the text is broken into chunks separated by `INTER_CHUNK_DELAY_MS` (50 ms): `claude` → 800 chars / 2 newlines per chunk; `codex` → 1000 chars; `gemini` → 1000 chars.

  Claude's numbers are exact rather than empirical. Its TUI replaces a paste with a `[Pasted text #N +M lines]` placeholder when the pasted string is over 800 characters **or** carries more than 2 newlines, so a chunk at or under both stays visible as text. (Inferred and unverified: the newline limit reads as `min(rows - 10, 2)`, so a pane under ~12 rows may collapse a chunk this logic considers safe.) Codex and Gemini remain empirical. The CLI is recognised three ways:
  - **By running process (primary, `app_detector/cli_detect.rs`).** At paste time the process table is scanned via libproc (`proc_listpids` / `proc_pidinfo`) and a process named `claude`/`codex`/`gemini` that is a *descendant of the foreground app's pid* is matched with high confidence. This is host-independent: it works even when the window title doesn't name the CLI — notably an IDE's integrated terminal (PhpStorm, IntelliJ, …), whose focused-window title is the project/file, not the CLI. Process enumeration needs no extra permission for the calling user's own processes. macOS only; non-macOS falls back to the title heuristic below. Node-wrapped CLIs (executable reported as `node`) aren't matched by name yet — they fall back too.
  - **By window title (fallback).** The foreground app's macOS bundle ID against a known terminal-like list (Terminal.app, iTerm2, Warp, Ghostty, Kitty, Alacritty, Hyper, WezTerm, VS Code, Cursor, Windsurf, JetBrains family incl. PhpStorm/CLion/RustRover, Android Studio) plus a case-insensitive substring match on the window title. A title names the focused window, so it outranks the machine-wide signal below.
  - **By "a CLI is running, and this is a pure terminal" (last resort).** A matching CLI process exists but is *not* a descendant of the foreground app (`Confidence::Low`), and the foreground bundle is a **pure terminal emulator** — `is_pure_terminal`, the emulator-only subset of the list above. This arm exists because descendancy is not a reliable test once a session-persistence daemon owns the pty, and the parent chain then never passes through the terminal you are looking at:
    - iTerm2 parents shells to `iTermServer` so sessions survive a restart; relaunching iTerm2 re-parents that server to launchd.
    - [Herdr](https://herdr.dev) is built around this: a background runtime owns the agent's pane so it outlives detach, network loss and reboots. It is a runtime, not an app, so there is no bundle ID to add — the terminal you attach from is the paste target, and this arm is what covers it.
    - `tmux` and `screen` have the same shape.

    Because we cannot tell *which* CLI owns the window here, this arm ignores the reported kind and applies the strictest known limit (Claude's). The kind attached to `Confidence::Low` is whichever matching process the scan reached first, and with both Claude Code and Codex running that is a coin flip — picking Codex's looser 1000-char limit would sail past the threshold that collapses a Claude paste. IDEs are deliberately excluded from this arm: their likelier paste target is the editor, where splitting one dictation would leave several undo steps, and a guess should not cost that.

  Non-terminal apps with none of these signals fall through to a single bulk paste.
- macOS Cmd+V via `CGEventPost` requires macOS **Accessibility** permission. The correction watcher uses Accessibility too, so this is a shared grant rather than a new one. There is no path to avoid Accessibility on modern macOS for keystroke synthesis; every alternative (`osascript "tell System Events to keystroke …"`, NSEvent simulation, AXUIElement post) ultimately routes through the same TCC check. `pipeline::output_text` pre-flights the grant with `is_accessibility_trusted()` and bails with `ACCESSIBILITY_REQUIRED` rather than letting the OS silently drop synthesised events when the grant is missing.

### Paste-landing detection

A dictation can have nowhere to land — the user clicked a browser tab/title bar, the desktop, the menu bar, or a non-editable element, so the synthesized Cmd+V is a no-op. In that case the dictation should stay on the clipboard (not be overwritten by the restore), and where we can tell, the capsule shows a "press ⌘V to paste" tip.

Detecting "landed" from outside the receiving app is **fundamentally limited on macOS** — only the app knows where the text went. Two signals are available, each with the same blind spot for browser web content, so the output path (for a **single, non-terminal** paste) combines them: one drives the tip, the other gates the destructive clipboard restore.

**1. Delayed-clipboard rendering → drives the tip.** `output/pasteboard_provider.m` (an Objective-C shim compiled by `build.rs`, mirroring `audio/mic_permission.m`) writes the dictation to `NSPasteboard` as a lazy `NSPasteboardItem` whose data is produced on demand by an `NSPasteboardItemDataProvider`, declaring `public.utf8-plain-text` plus a private `com.opentypeless.dictation` sentinel. `clipboard.rs::paste_single_detect` writes it, synthesizes Cmd+V, waits `LANDED_TIMEOUT_MS` (400 ms), then reads whether the plain type was requested (`PasteOutcome::landed`). **Not read → genuine no-target** (nothing consumed it: menu bar, desktop, a native non-editable control) → materialize the text concretely, leave it on the clipboard, emit `output:no_target` → tip. This is reliable for native contexts. It is **not** reliable for browsers: Chrome/Electron read the clipboard on Cmd+V even when the paste lands nowhere (read-and-discard), so a read there says `landed = true` whether or not the text went anywhere — the tip simply stays silent in browsers (`Needs confirmation`: empirically observed against Chrome).

**2. Accessibility (positive-only) → gates the restore.** Because a read alone can't be trusted (browser read-and-discard), restoring the user's previous clipboard on a read would risk overwriting — and losing — the dictation. So restore happens **only** when `correction::focused_editable_present()` confirms a focused editable text element (`AXTextField`/`AXTextArea`/`AXSearchField`/`AXComboBox`/`AXSecureTextField`). AX is blind to browser contenteditable (reports nothing), so those return `false` → no restore → the dictation is kept. Net: `read && editable && no-clipboard-manager → restore` (the confident native/web-input landing); every other read leaves the dictation on the clipboard, recoverable with a manual ⌘V and never lost.

**Clipboard managers** (Maccy/Paste/Raycast/…) mirror the pasteboard and would read the plain type, faking a "landed". A history-mirroring reader also reads *every* type including the private sentinel; when the sentinel is read, the process-sticky `SENTINEL_SEEN` flag is set and we stop restoring for the session, so a dictation is never destroyed by an untrustworthy signal.

**Outcomes:** native no-target (menu bar / desktop / non-editable) → tip + keep; real text field → restore + no tip; browser/contenteditable → silent + keep (no loss). **Terminals and any chunked (multi) paste skip detection entirely** — reliable targets (daily CLI use), eager write+restore, always `landed = true` (no tip). Non-macOS has no detection yet and always reports `landed = true`.

## Important Invariants

- `output_text()` normalizes through `normalize_for_output(text, replaced_selection)`. An **inserted** dictation is trimmed and gets a single trailing space, so successive dictations don't glue together. Text that **replaces a selection** is trimmed only: the paste has to occupy the selected range exactly, and an appended space would nudge the following word out of place on every edit. History stores the un-normalized text.
- LLM polish output is batched: the capsule renders streamed `llm:chunk` events for a live indicator, but the paste only fires once polish completes.
- `pipeline_lock` serializes `start()` and `stop()`.
- `abort()` sets the abort flag, drops the audio handle, notifies `stt_done`, clears accumulated text, and forces `Idle`.
- **Edit mode requires the Accessibility preflight.** No clipboard-derived selection is ever passed to the LLM: a copy keystroke cannot distinguish a real selection from an app copying the current line, and guessing wrong silently rewrites text the user did not select. See [Selected-Text Capture](#selected-text-capture).
- **A failed LLM call must not paste over a selection.** The plain-dictation path falls back to pasting the raw transcript when polish fails, which is the right trade there. The selection-replacing path has no such fallback: pasting the raw transcript would overwrite the selected text with the literal words of the instruction ("fix the grammar") and destroy what the user was editing. It leaves the selection untouched and surfaces the error instead.
- The correction watcher is skipped when a selection was replaced. It anchors on the span of text it believes was just typed, and an edit that rewrites a whole paragraph gives it no such span.
- macOS Accessibility permission is checked through raw FFI (`AXIsProcessTrusted`). It is required for output (keystroke synthesis of Cmd+V) and for the correction watcher (focused-field reads). A single grant covers both.
- Tray tooltip and capsule UI both subscribe to `pipeline:state`; consider both when changing state semantics.
- Retry stops at the last point where output is still invisible: the STT handshake, the file upload, the LLM request head. Anything past that — audio already sent, chunks already emitted — must not retry.

## Needs confirmation

- Whether the silent-retry choice holds in practice — no field data yet on how often a retry fires or how long users perceive the added wait. See [Transient Failure Retry](#transient-failure-retry).
- `AppConfig.max_recording_seconds` (default 30) is enforced in code; the precise enforcement path should be documented after a focused code review.
