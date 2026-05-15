# Architecture Overview

OpenTypeless is a Tauri 2 desktop app. The user starts recording with a global hotkey or tray action, speaks, and the app transcribes, optionally polishes or translates, then outputs text into the foreground app.

Stack summary lives in [`CLAUDE.md`](../../CLAUDE.md). This page covers boundaries and runtime shape.

Evidence: `src-tauri/tauri.conf.json`, `src-tauri/src/pipeline.rs`, `src/App.tsx`.

## Main Flow

```text
microphone → audio capture → STT provider → raw transcript → LLM polish → keyboard / clipboard output
```

Detail: [Pipeline](pipeline.md). Provider abstractions: [Providers](providers.md). Frontend wiring: [Frontend ↔ Backend](frontend-backend.md).

## Module Boundaries

| Path | Responsibility |
| --- | --- |
| `src/` | React frontend bundle (main and capsule windows share it). |
| `src/components/Capsule/` | Floating recording widget. |
| `src/components/Settings/` | Settings panes and provider configuration UI. |
| `src/stores/appStore.ts` | App config + UI state (Zustand). |
| `src-tauri/src/lib.rs` | Tauri setup, command registry, tray, hotkey wiring. |
| `src-tauri/src/pipeline.rs` | Core recording pipeline (singleton `PipelineHandle`). |
| `src-tauri/src/stt/` | STT provider implementations and factory. |
| `src-tauri/src/llm/` | LLM provider implementations and prompt builder. |
| `src-tauri/src/output/` | Clipboard-based text output with per-target chunking. |
| `src-tauri/src/storage/` | Config (`tauri-plugin-store`) and SQLite stores. |
| `src-tauri/src/audio/` | Microphone capture (`cpal`). |
| `src-tauri/src/app_detector/` | Foreground app detection / classification. |

## Windows

`src-tauri/tauri.conf.json` defines two windows:

- `main` — settings, history, home, onboarding. Starts hidden.
- `capsule` — small transparent always-on-top widget loaded from `index.html#capsule`.

`src/App.tsx` switches synchronously on `window.location.hash`, so the same JS bundle renders either app without a race during startup.

On macOS the app runs as a status-bar utility: `lib.rs` sets the activation policy to `Accessory` at startup, so OpenTypeless has no Dock icon and is reached through the menu-bar tray. This is required so the capsule can overlay other apps' fullscreen Spaces — see [Frontend ↔ Backend → macOS capsule overlay](frontend-backend.md#macos-capsule-overlay-mechanics) for the full set of macOS-specific window mechanics.

## Inferences

- The app is local-first BYOK only — no cloud account, subscription, telemetry, or auto-update. All STT/LLM calls go directly from the user's machine to the provider they configured.
- The Rust backend is the source of truth for pipeline state and provider execution (inferred from Tauri command ownership and the singleton `PipelineHandle`).

## Needs confirmation

- No formal layering rules beyond the module boundaries above.
- Linux foreground-app detection currently falls back to a default context; the intended Linux behavior should be confirmed by a maintainer.
