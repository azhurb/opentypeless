# Pipeline

`src-tauri/src/pipeline.rs` orchestrates the core flow:

```text
mic -> STT -> LLM polish -> keyboard/clipboard output
```

`PipelineHandle` is a Tauri-managed singleton. It stores the current `PipelineState`, audio handle, accumulated transcript, abort flag, preloaded config/context/dictionary, selected text, recording start time, shared HTTP client, and a `pipeline_lock`.

## States

`PipelineState` values are:

- `Idle`
- `Recording`
- `Transcribing`
- `Polishing`
- `Outputting`

State changes emit `pipeline:state` to the frontend and update tray tooltip/menu state.

## Start Flow

When recording starts:

1. `pipeline_lock` serializes setup.
2. State changes from `Idle` to `Recording`.
3. Config, current app context, and dictionary are loaded.
4. STT API configuration is built.
5. STT provider connects before audio capture starts.
6. Audio capture starts and streams chunks to STT.
7. Partial and final transcript events are emitted to the frontend.

The STT API key guard is skipped for `cloud`, where the session token is used.

## Stop Flow

When recording stops:

1. State changes from `Recording` to `Transcribing`.
2. If selected-text mode is enabled, selected text is captured after a short delay so hotkey modifiers can be released.
3. Audio capture stops.
4. The pipeline waits for STT finalization.
5. If polish is enabled, final text is sent to the LLM provider.
6. Output is sent by keyboard simulation or clipboard paste.
7. History is stored.
8. State returns to `Idle`.

## Important Invariants

- `pipeline_lock` serializes `start()` and `stop()` so quick press-release in hold mode cannot make `stop()` observe partially initialized setup.
- `abort()` sets `abort_flag`, drops the audio handle, notifies `stt_done`, clears accumulated text, and forces `Idle`.
- Selected-text capture simulates Cmd/Ctrl+C only after `SELECTED_TEXT_CAPTURE_DELAY_MS`.
- Clipboard content is restored after selected-text capture.
- On macOS, if Cmd+C does not change the clipboard, selected text is ignored to avoid passing stale clipboard content to the LLM.
- macOS Accessibility permission is checked through raw FFI because `enigo` silently drops events without it.
- Tray tooltip and capsule state are both affected by `pipeline:state`; think through both when changing pipeline states.

## Events

Observed pipeline-related events include:

- `pipeline:state`
- `pipeline:error`
- `audio:volume`
- `stt:partial`
- `stt:final`
- `llm:chunk`

## Needs confirmation

- User-facing retry semantics after STT or LLM failure are not documented separately from current code behavior.
- Maximum recording behavior is configured in `AppConfig`, but the precise enforcement path should be documented after code review.
