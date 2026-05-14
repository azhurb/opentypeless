# Frontend And Backend Wiring

The frontend talks to Rust through Tauri commands and listens to Rust-emitted events. This page covers the wiring; pipeline state semantics are in [Pipeline](pipeline.md).

Evidence: `src/lib/tauri.ts`, `src-tauri/src/lib.rs`, `src/hooks/useTauriEvents.ts`, `src/App.tsx`.

## Tauri Commands

Rust commands are registered in the `tauri::generate_handler![...]` block at the bottom of `src-tauri/src/lib.rs`. TypeScript wrappers live in `src/lib/tauri.ts`.

**Rule:** every `#[tauri::command]` must be both registered in `generate_handler!` and called via either a wrapper in `src/lib/tauri.ts` or a direct `invoke()`. Adding one without the other is a common integration bug.

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
- Corrections: `correction_undo`.

A generated command/signature reference would be a good fit for [`docs/generated/`](../generated/README.md); none exists yet.

## Events

Rust emits events with `app_handle.emit(...)` / `window.emit(...)`. The frontend subscribes through `useTauriEvents`. Cross-check with [Pipeline → Events](pipeline.md#events) when changing pipeline state.

Event names emitted by the backend:

- Pipeline: `pipeline:state`, `pipeline:error`, `pipeline:target_app`.
- Audio/STT/LLM streams: `audio:volume`, `stt:partial`, `stt:final`, `llm:chunk`.
- Corrections: `correction:suggest` (emitted to the capsule window only).
- Tray: `tray:settings`, `tray:history`, `tray:about`.

Event payload contracts are not centrally documented yet; reading the emit sites is the source of truth.

## State

- App state and persisted config: `src/stores/appStore.ts` (Zustand).

## Two Windows, One Bundle

- `main` renders `MainApp`.
- `capsule` is loaded with `#capsule` and renders `CapsuleApp`.

`src/App.tsx` reads `window.location.hash` synchronously to avoid rendering the wrong app during startup.

The capsule is shown via `useCapsuleResize` in the order `setSize` → `setPosition` → `show`. `requestAnimationFrame` is intentionally avoided because WKWebView pauses rAF in hidden macOS windows (see `src/App.tsx` comment).

`useCapsuleResize` repositions the capsule on the monitor under the cursor every time the pipeline transitions from idle to active (and on first mount). It calls `cursorPosition()` then `monitorFromPoint(...)`, but converts the cursor to logical coords first: tao's `cursorPosition()` returns physical coordinates scaled by the **primary** monitor's factor, while `monitorFromPoint` checks against `CGDisplayBounds` which is logical, so on Retina + multi-monitor setups the lookup either fails or hits the wrong screen without the conversion.

### macOS Capsule Overlay Mechanics

To make the capsule render correctly across all the situations users hit on macOS — auto-hide, multi-monitor, other apps' fullscreen Spaces — four things have to line up. They are all fragile and easy to regress, so they are documented together here.

1. **Background throttling disabled.** The capsule window has `backgroundThrottling: "disabled"` in `tauri.conf.json`. Without it, when the user enables `capsule_auto_hide`, macOS WebKit suspends the capsule's WebContent process ~480 s after the window is hidden. A suspended WebContent stops processing IPC events, so the `pipeline:state` event emitted on the next hotkey press never reaches the JS listener that calls `win.show()` — the overlay silently never appears. Disabling background throttling sets WebKit's `inactiveSchedulingPolicy` to `None`, keeping the JS runtime alive while the window is hidden.
2. **Activation policy = Accessory.** `lib.rs` calls `set_activation_policy(ActivationPolicy::Accessory)` during setup. macOS excludes a `.regular` app's windows from other apps' fullscreen Spaces regardless of any window flag; only `.accessory` (status-bar) apps may join. The visible cost is that OpenTypeless has no Dock icon on macOS — the menu-bar tray is the only entry point. This is also why Wispr Flow ships a separate `LSUIElement = true` helper bundle to host its overlay.
3. **NSPanel class swap + `NonactivatingPanel` style.** `configure_capsule_collection_behavior` in `lib.rs` swaps the capsule's `NSWindow` to `NSPanel` via `object_setClass` and ORs `NSWindowStyleMaskNonactivatingPanel` (1 << 7) into the style mask. macOS only renders an overlay inside a foreign fullscreen Space when the window is an `NSPanel` with this style; a plain `NSWindow` is hidden. Safe because `NSPanel` adds no extra ivars over `NSWindow`.
4. **Collection behavior + window level.** Same function ORs `NSWindowCollectionBehaviorFullScreenAuxiliary` (1 << 8) into the collection behavior (preserving `CanJoinAllSpaces`, which Tauri's `visibleOnAllWorkspaces: true` already set), and raises the level to `NSPopUpMenuWindowLevel` (101). Tauri's `alwaysOnTop` only sets `NSFloatingWindowLevel` (3), which sits below a fullscreen app's content; `NSPopUpMenuWindowLevel` is what status-item popovers use and reliably overlays fullscreen.

If any of these four are removed, the capsule will look fine on a single non-fullscreen display but silently misbehave in one of the harder cases.

## Caches For Hot Paths

`HotkeyModeCache` and `CloseToTrayCache` live in Rust because reading config from `tauri-plugin-store` is async and would block hotkey/window-close handlers. They are refreshed on every successful `update_config`.

## Needs confirmation

- Event payload contracts are not documented beyond the emit sites in code.
- A generated command + event reference would prevent the "wrapper exists but command isn't registered" class of bug; no generator exists yet.
