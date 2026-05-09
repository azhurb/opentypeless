# Frontend And Backend Wiring

The frontend talks to Rust through Tauri commands and listens to Rust-emitted events. This page covers the wiring; pipeline state semantics are in [Pipeline](pipeline.md).

Evidence: `src/lib/tauri.ts`, `src-tauri/src/lib.rs`, `src/hooks/useTauriEvents.ts`, `src/App.tsx`.

## Tauri Commands

Rust commands are registered in the `tauri::generate_handler![...]` block at the bottom of `src-tauri/src/lib.rs`. TypeScript wrappers live in `src/lib/tauri.ts`.

**Rule:** every `#[tauri::command]` must be both registered in `generate_handler!` and called via either a wrapper in `src/lib/tauri.ts` or a direct `invoke()` (e.g. `set_session_token` is invoked directly from `src/stores/authStore.ts`). Adding one without the other is a common integration bug.

Current command groups (grep-verified against `generate_handler!`):

- Pipeline: `start_recording`, `stop_recording`, `abort_recording`.
- Permissions: `check_accessibility_permission`, `request_accessibility_permission`.
- Config: `get_config`, `update_config`.
- Provider checks: `test_stt_connection`, `test_llm_connection`, `bench_stt_connection`, `bench_llm_connection`.
- LLM metadata: `fetch_llm_models`.
- History: `get_history`, `clear_history`.
- Dictionary: `get_dictionary`, `add_dictionary_entry`, `remove_dictionary_entry`.
- Hotkey: `update_hotkey`, `pause_hotkey`, `resume_hotkey`.
- Auto-start: `set_auto_start`.
- Auth/cloud: `set_session_token`.

A generated command/signature reference would be a good fit for [`docs/generated/`](../generated/README.md); none exists yet.

## Events

Rust emits events with `app_handle.emit(...)` / `window.emit(...)`. The frontend subscribes through `useTauriEvents`. Cross-check with [Pipeline → Events](pipeline.md#events) when changing pipeline state.

Event names emitted by the backend:

- Pipeline: `pipeline:state`, `pipeline:error`, `pipeline:target_app`.
- Audio/STT/LLM streams: `audio:volume`, `stt:partial`, `stt:final`, `llm:chunk`.
- Tray: `tray:settings`, `tray:history`, `tray:about`.
- Navigation: `navigate` (sent from the tray "account" action).

Event payload contracts are not centrally documented yet; reading the emit sites is the source of truth.

## State

- App state and persisted config: `src/stores/appStore.ts` (Zustand).
- Cloud auth/session: `src/stores/authStore.ts`.

## Two Windows, One Bundle

- `main` renders `MainApp`.
- `capsule` is loaded with `#capsule` and renders `CapsuleApp`.

`src/App.tsx` reads `window.location.hash` synchronously to avoid rendering the wrong app during startup.

The capsule is shown via `useCapsuleResize` in the order `setSize` → `setPosition` → `show`. `requestAnimationFrame` is intentionally avoided because WKWebView pauses rAF in hidden macOS windows (see `src/App.tsx` comment).

## Caches For Hot Paths

`HotkeyModeCache` and `CloseToTrayCache` live in Rust because reading config from `tauri-plugin-store` is async and would block hotkey/window-close handlers. They are refreshed on every successful `update_config`.

## Needs confirmation

- Event payload contracts are not documented beyond the emit sites in code.
- A generated command + event reference would prevent the "wrapper exists but command isn't registered" class of bug; no generator exists yet.
