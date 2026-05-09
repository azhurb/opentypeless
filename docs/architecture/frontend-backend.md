# Frontend And Backend Wiring

The frontend talks to Rust through Tauri commands and listens to Rust-emitted events.

Evidence: `src/lib/tauri.ts`, `src-tauri/src/lib.rs`, `src/hooks/useTauriEvents.ts`, `src/App.tsx`.

## Tauri Commands

Rust commands are registered in the `tauri::generate_handler![...]` block in `src-tauri/src/lib.rs`.

TypeScript wrappers live in `src/lib/tauri.ts`.

Rule: keep Rust commands and TypeScript wrappers in sync. Adding a `#[tauri::command]` without a wrapper, or adding a wrapper without registering the command, is a common integration bug.

Current command groups:

- Pipeline: start, stop, abort.
- Permissions: check/request macOS Accessibility permission.
- Config: get/update config.
- Provider checks: STT/LLM connection tests and latency benchmarks.
- LLM metadata: fetch model list.
- History: list and clear entries.
- Dictionary: list, add, remove entries.
- Hotkey: update, pause, resume.
- Auto-start: enable or disable.
- Auth/cloud: set session token.

## Events

Rust emits events with `app_handle.emit(...)`. The frontend listens through `useTauriEvents`.

Known event names include:

- `pipeline:state`
- `pipeline:error`
- `audio:volume`
- `stt:partial`
- `stt:final`
- `llm:chunk`
- `tray:settings`
- `tray:history`
- `tray:about`
- `navigate`

## State

Frontend app state is centralized in `src/stores/appStore.ts` with Zustand.

Cloud auth/session state is in `src/stores/authStore.ts`.

## Two Windows, One Bundle

Both windows use the same JS bundle:

- `main` renders `MainApp`.
- `capsule` loads with `#capsule` and renders `CapsuleApp`.

`src/App.tsx` checks `window.location.hash` synchronously. This avoids a race where the capsule could render the wrong app during startup.

The capsule is shown through `useCapsuleResize`, which performs setSize, setPosition, then show. Existing comments say requestAnimationFrame is avoided because WKWebView pauses rAF in hidden macOS windows.

## Caches For Hot Paths

`HotkeyModeCache` and `CloseToTrayCache` live in Rust because reading config from `tauri-plugin-store` is async and would block hotkey/window-close handlers. They are updated whenever `update_config` runs.

## Needs confirmation

- Event payload contracts are not centrally documented beyond current code.
- There is no generated command/event reference yet.
