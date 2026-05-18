# Dictation startup latency

Status: **in user testing** (initial fixes landed, follow-ups deferred until we have real-world data).

## Problem

When the user holds the dictation hotkey and starts speaking immediately, the
transcript sometimes drops the first few words. The effect is most visible on
short utterances ("hello world", "yes please") and is highly variable —
sometimes the same key-press feels instant, sometimes it eats half a second.

## Root cause

The pre-fix `PipelineHandle::start()` ran a fully sequential chain on the
hotkey-press side before opening the `cpal` input stream. On macOS the chain
included three contributors that each cost real wall-clock time:

| Step | Implementation | Typical cost |
|---|---|---|
| `app_detector::detect_current_app()` | 3 sequential `osascript "tell application System Events ..."` shell-outs | 150–450 ms |
| `stt::Provider::connect()` (streaming) | full WebSocket handshake (TCP + TLS + HTTP Upgrade) to Deepgram / AssemblyAI | 100–500 ms |
| `AudioCaptureHandle::start()` | `cpal` device open + sample-rate negotiation + `stream.play()` | 50–300 ms |

Total dead window between key-down and the first audio sample landing in the
channel: roughly **300 ms – 1.2 s** on macOS, with variance dominated by
`osascript` cold-start and network jitter. The capsule UI emitted
`pipeline:state = Recording` at the top of `start()`, before any of this had
happened — which is why the missed words felt surprising.

For non-streaming STT providers (Whisper-compat: OpenAI / Groq / SiliconFlow /
GLM-ASR) the `connect()` cost is a no-op, so only the `osascript` and `cpal`
contributors are in play; the gap is smaller but still meaningful.

The pre-existing `pipeline::PipelineHandle::pre_warm()` only issues HEAD
requests to the STT and LLM HTTP endpoints. It warms DNS + TLS connection
pools but does **not** keep a WebSocket open, does not pre-open the mic
device, and does not cache the foreground app.

## Fixes shipped

### #1 — Native macOS foreground-app detection

`src-tauri/src/app_detector/mod.rs`

Replaced the three `osascript` calls with:

- **`NSWorkspace.sharedWorkspace.frontmostApplication`** via the Objective-C
  runtime (`objc_msgSend` through the `objc` dylib) to read `localizedName`,
  `bundleIdentifier`, and `processIdentifier`. Reuses the FFI pattern from
  `src-tauri/src/lib.rs`'s window-styling code.
- **`AXUIElementCreateApplication(pid) → AXFocusedWindow → AXTitle`** via
  ApplicationServices for the window title. Gated on
  `pipeline::is_accessibility_trusted()` — the same Accessibility grant that
  paste output already needs. CFString helpers (`cfstr`, `cf_string_to_rust`)
  mirror those in `src-tauri/src/correction/ax_macos.rs`.

Failure paths return empty strings / `None`, identical graceful-degradation
contract to the previous osascript-error path. The chunker's bundle-ID
matching keeps working unchanged; the CLI-title substring branch only
disables when AX is unavailable.

Expected savings: ~150–450 ms per recording on macOS.

### #2 — Reorder `start()` so audio capture begins first

`src-tauri/src/pipeline.rs`

`AudioCaptureHandle::start(config)` now runs immediately after the Idle →
Recording state transition, **before** config load, app detection, dictionary
fetch, and STT connect. The `mpsc::channel::<Vec<u8>>(200)` in
`src-tauri/src/audio/capture.rs` (~4 s of 20 ms chunks) absorbs samples while
the rest of setup runs, then the STT forwarder task flushes the pre-buffer
the moment the WebSocket handshake completes.

Other changes in the same edit:

- `recording_start = Instant::now()` moves up, so the
  `pipeline:timing.recording_ms` metric now reflects real capture duration.
- Error / abort paths (audio open failure, empty API key, STT connect
  failure, abort during setup) all go through a new `cleanup_failed_start()`
  helper that stops audio, clears preloaded slots, and transitions back to
  Idle. Removes ~50 lines of repeated cleanup code from the prior layout.
- The volume-monitoring task is now spawned at the new audio-first position;
  it only reads `audio_handle` and `state`, both available immediately.

What does NOT change:

- The STT forwarder task body — same select-loop, same provider semantics.
- The capsule UI events — `pipeline:state = Recording` still fires at the
  top of `start()`. Distinct "preparing → recording" UX is on the follow-up
  list (#4 below).
- Provider-level behaviour for Deepgram WS, AssemblyAI WS, and Whisper-compat
  buffer-and-POST.

## How to test

1. Hold the hotkey and immediately say a short phrase ("hello", "yes please",
   "open the file"). The first word should now appear in the transcript on
   every press. Try this both on the streaming providers (Deepgram by
   default) and on Whisper-compat (OpenAI / Groq) by switching in Settings.
2. Watch the `[Pipeline Timing]` log lines. `recording_ms` should match what
   you perceive as your speaking duration, not "duration minus the dead
   window."
3. Paste-chunking sanity check: dictate into Claude CLI / Codex CLI / VS Code
   terminal. Bundle-ID-based chunking should still kick in (Terminal.app,
   iTerm2, Warp, Ghostty, VS Code, Cursor, etc.); title-based chunking
   (claude / codex / gemini substring) should still kick in when macOS
   Accessibility is granted.
4. Permission edge cases:
   - Revoke Accessibility in System Settings → recording still works; only
     `window_title` falls back to empty, and only AX-dependent paste paths
     emit `ACCESSIBILITY_REQUIRED` as before.
   - Revoke Microphone → `MICROPHONE_DENIED` still surfaces before audio is
     touched.
5. Rapid press-release of the hotkey in hold mode should still work — the
   `pipeline_lock` invariant from `pipeline.rs:184-189` is preserved.

## Deferred follow-ups

If user testing shows the first-words problem is **still** present after #1
and #2, pick up these in roughly increasing-effort order:

- **#3 — Parallelise the remaining setup.** Run `load_config`,
  `dictionary.words()`, and `provider.connect()` concurrently via
  `tokio::try_join!`. Currently sequential; total post-fix gain is probably
  only tens of ms because the config + dictionary reads are sub-ms in
  practice, but it's basically free.
- **#4 — Distinct "Preparing" vs "Recording" capsule UI state.** Only emit
  `pipeline:state = Recording` once the cpal stream has produced its first
  chunk (e.g. via a `tokio::sync::oneshot` signalled from the volume task on
  first non-default volume read, or via an explicit cpal-callback
  notification). This addresses the *perception* gap rather than the
  capture gap: users who see "Recording" instantly trust the indicator and
  start speaking before the hardware is fully open. The Wispr RE notes in
  `.scratch-research/findings/01-ipc-schema.md:43-49` show the comparable
  product models this as two distinct IPC messages (`DictationStart` vs
  `RecordingStarted`).
- **#5 — Persistent STT WebSocket while idle.** Keep a single Deepgram /
  AssemblyAI WS open across recordings, with automatic reconnect on idle
  timeout. Removes the connect cost (~100–500 ms) from the hot path
  entirely. Watch out for: server-side idle timeouts (Deepgram is generous
  but not unlimited), reconfiguration when the user changes language pins or
  smart-format settings, and clean handover from one recording to the next
  without losing the in-flight transcript.

## Files touched

- `src-tauri/src/app_detector/mod.rs` — native FFI for macos_detect.
- `src-tauri/src/pipeline.rs` — reordered `start()`, added
  `cleanup_failed_start()` helper.
- `docs/plans/active/dictation-startup-latency.md` — this note.
- `docs/plans/active/README.md` — pointer to this note.

No new dependencies, no frontend changes, no provider-trait changes.

## Move-to-completed criteria

After the user has tested for some real-world dictation sessions:

- If the first-words problem is no longer reported → move this note to
  `docs/plans/completed/` and close the loop.
- If it persists → pick up follow-up #4 or #5 (the most impactful remaining
  levers) and update this note in place.
