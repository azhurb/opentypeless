# Architecture Overview

OpenTypeless is a Tauri 2 desktop app for AI voice input. The user starts recording with a global hotkey or tray action, speaks, and the app transcribes, optionally polishes or translates, then outputs text into the foreground app.

Evidence: `README.md`, `src-tauri/tauri.conf.json`, `src-tauri/src/pipeline.rs`, `src/App.tsx`.

## Runtime Shape

- Frontend: React 19, TypeScript, Tailwind 4, Vite, Zustand, i18next.
- Backend: Rust 2021, Tokio, Tauri 2.
- Audio capture: `cpal`.
- Text output: `enigo` for keyboard simulation, `arboard` for clipboard.
- Local storage: `tauri-plugin-store` for config and `rusqlite` for history/dictionary.
- HTTP and streaming: `reqwest` and `tokio-tungstenite`.

## Main Flow

```text
Microphone -> audio capture -> STT provider -> raw transcript -> LLM polish -> keyboard/clipboard output
```

The central orchestrator is `src-tauri/src/pipeline.rs`. It coordinates recording lifecycle, selected-text capture, provider calls, state events, and output.

## Main Boundaries

- `src/` contains the React frontend.
- `src-tauri/src/` contains the Rust backend.
- `src-tauri/src/pipeline.rs` owns the core recording pipeline.
- `src-tauri/src/stt/` contains STT provider implementations.
- `src-tauri/src/llm/` contains LLM provider implementations and prompt building.
- `src-tauri/src/output/` contains keyboard and clipboard output modes.
- `src-tauri/src/storage/` contains config, history, and dictionary storage.
- `src/components/Capsule/` contains the floating recording widget.
- `src/components/Settings/` contains provider and app configuration UI.

## Windows

`src-tauri/tauri.conf.json` defines two windows:

- `main`: settings, history, home, onboarding, account, and upgrade views. It starts hidden.
- `capsule`: small transparent always-on-top widget loaded from `index.html#capsule`.

`src/App.tsx` synchronously switches on `window.location.hash` so the same bundle renders either the main app or capsule app.

## Inferences

- The app is designed around local-first BYOK operation with optional cloud subscription mode. This is inferred from `README.md`, `storage::AppConfig` defaults, provider code, and auth/store code.
- The Rust backend is the source of truth for pipeline state and provider execution. This is inferred from Tauri command ownership and the `PipelineHandle` singleton.

## Needs confirmation

- Long-term architectural layering rules are not formally defined beyond current module boundaries.
- Linux active-app detection currently falls back to a default context; intended Linux behavior needs maintainer confirmation.
