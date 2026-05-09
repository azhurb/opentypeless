# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

OpenTypeless is a Tauri 2 desktop app (Windows/macOS/Linux) for AI voice input: hold a hotkey, speak, and the app transcribes via an STT provider, polishes via an LLM, and types the result into the foreground app.

- Frontend: React 19 + TypeScript + Tailwind 4 + Vite, Zustand for state, i18next for translations.
- Backend: Rust 2021, Tokio, `cpal` for audio capture, `enigo` for keyboard simulation, `arboard` for clipboard, `rusqlite` (bundled) for SQLite, `reqwest` for HTTP, `tokio-tungstenite` for streaming STT.

## Commands

```bash
# Dev (runs Vite on :1420 + Tauri shell)
npm run tauri dev

# Production build (output: src-tauri/target/release/bundle/)
npm run tauri build

# Frontend checks (mirror CI in .github/workflows/ci.yml)
npx tsc --noEmit
npx eslint src/
npx prettier --check src/
npx vitest run                  # all tests
npx vitest run path/to/file     # single file
npx vitest -t "pattern"         # filter by test name

# Rust checks
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml test_parse_hotkey_ctrl_slash  # single test
```

Build with `--features devtools` (or `npm run tauri dev -- --features devtools`) to auto-open WebKit devtools for both windows.

Override the cloud backend at build time:

```bash
VITE_API_BASE_URL=https://my.example.com API_BASE_URL=https://my.example.com npm run tauri build
```

## Architecture

### Pipeline (the core flow)

`src-tauri/src/pipeline.rs` orchestrates: **mic → STT → LLM polish → keyboard/clipboard output**. The `PipelineHandle` is a single Tauri-managed singleton holding atomic state (`PipelineState`: Idle/Recording/Transcribing/Polishing/Outputting), an audio handle, accumulated text, and an abort flag. State changes emit `pipeline:state` events to the frontend; the same handle drives both the global hotkey and the tray "Start/Stop Recording" menu item.

Key invariants:
- `pipeline_lock` (a `tokio::sync::Mutex`) serializes `start()`/`stop()` so a quick press-release in hold mode can't have `stop()` race ahead of `start()` setup.
- `abort()` (called by the capsule's abort button) sets `abort_flag`, drops the audio handle, notifies `stt_done`, clears accumulated text, and forces `Idle` — bypassing the normal STT-finalize wait.
- Selected-text capture (`selected_text_enabled`) simulates Cmd/Ctrl+C *after* `SELECTED_TEXT_CAPTURE_DELAY_MS` to wait for hotkey modifiers to release; clipboard is always restored. On macOS, if Cmd+C had no effect (e.g., no Accessibility permission), `selected == backup` and the function returns `None` to avoid feeding stale clipboard content to the LLM.
- `is_accessibility_trusted()` / `request_accessibility_permission()` use raw FFI to `AXIsProcessTrusted[WithOptions]` because `enigo` silently drops events on macOS without this permission.

### Provider abstraction

Both STT and LLM use a trait + factory pattern. To add a provider, implement the trait and add a match arm to `create_provider`:

- `src-tauri/src/stt/mod.rs` — `SttProvider` trait, dispatched by string name in `create_provider()`. Cloud, AssemblyAI, and Deepgram are bespoke; `glm-asr`, `openai-whisper`, `groq-whisper`, `siliconflow` all share `WhisperCompatProvider` (configured via `WhisperCompatConfig`). Streaming providers emit `TranscriptEvent::{Partial, Final, SpeechStarted, SpeechEnded, Error}`; file-based providers return the final transcript from `disconnect()`.
- `src-tauri/src/llm/mod.rs` — `LlmProvider` trait. `OpenAiProvider` is OpenAI-compatible (used for OpenAI, DeepSeek, Gemini, Claude via OpenRouter, Ollama, etc., distinguished only by `base_url` + `model`). `CloudProvider` proxies via `{API_BASE_URL}/api/proxy/llm` with the session bearer token. Polish prompts live in `src-tauri/src/llm/prompt.rs` and condition on `AppType` + dictionary + selected-text context.

When adding a provider, also: (1) add it to the `SttProvider`/`LlmProvider` union in `src/stores/appStore.ts`, (2) wire up the connection-test and benchmark match arms in `src-tauri/src/lib.rs` (`test_stt_connection`, `bench_stt_connection`, etc.), (3) add UI option in `src/components/Settings`.

### Two-window app

`src-tauri/tauri.conf.json` defines two windows that share the same JS bundle:

- `main` — main app UI (Settings, History, Home, Onboarding, Account, Upgrade), starts hidden.
- `capsule` — a small, transparent, frameless, always-on-top floating widget. The same `index.html` is loaded with `#capsule` in the URL; `src/App.tsx` switches synchronously on `window.location.hash` (no race) to render `<CapsuleApp>` vs `<MainApp>`.

The capsule is shown via a setSize → setPosition → show sequence (see `useCapsuleResize`) — never via rAF, because WKWebView pauses requestAnimationFrame in hidden windows on macOS.

### Frontend ↔ backend wiring

- All Rust commands the frontend can call are listed in the `tauri::generate_handler![...]` block at the bottom of `src-tauri/src/lib.rs`. The TypeScript wrappers live in `src/lib/tauri.ts`. Keep these in sync — adding a `#[tauri::command]` without a wrapper (or vice versa) is a common bug.
- Events flow Rust → frontend via `app_handle.emit(...)`: `pipeline:state`, `pipeline:error`, `tray:settings`, `tray:history`, `tray:about`, `navigate`. The capsule and main window both listen via `useTauriEvents`.
- `HotkeyModeCache` and `CloseToTrayCache` exist because reading config from `tauri-plugin-store` is async and would block hotkey/window-close handlers — both caches are updated whenever `update_config` runs.

### Storage

- **Config** (`AppConfig`) — `tauri-plugin-store`, file `settings.json` in the OS app-data dir, key `app_config`. `ConfigManager` (in `src-tauri/src/storage/mod.rs`) caches the deserialized config in-memory. Window position/size is also stored here under `window_state`.
- **History + Dictionary** — SQLite at `<app_data_dir>/opentypeless.db`. Schema in `src-tauri/migrations/001_init.sql`. Both are opened directly via `rusqlite` (bundled feature, so no system sqlite needed) — the `tauri-plugin-sql` plugin is registered for the frontend but the Rust side talks to SQLite directly.

### Cloud (Pro) mode

When the user picks `cloud` as the STT or LLM provider, requests go through `{API_BASE_URL}/api/proxy/{stt,llm}` with a Better Auth session bearer token. The token flow:

1. Frontend logs in via `better-auth` (see `src/lib/auth-client.ts`, `src/stores/authStore.ts`).
2. After login, frontend calls `set_session_token(token)` which stores it in `SessionTokenStore` (an `Arc<Mutex<String>>` Tauri state).
3. Rust providers read this when constructing the cloud STT/LLM provider.

Connection-test commands hit `/api/subscription/status` and verify `plan == "pro"` before reporting success. Deep-link auth callbacks come in via the `opentypeless://` scheme (see `src/lib/deep-link.ts`); `tauri-plugin-single-instance` ensures only one app instance and forwards URLs to the running one.

### Hotkey handling

`parse_hotkey()` in `src-tauri/src/lib.rs` parses strings like `"Ctrl+Shift+A"` / `"Alt+/"` into `tauri-plugin-global-shortcut` `Shortcut`s. `build_shortcut_handler` dispatches based on `HotkeyModeCache` (`"hold"` vs `"toggle"`). `pause_hotkey`/`resume_hotkey` exist so the Settings UI can capture a new key combo without the old hotkey firing during recording.

## Conventions

- TypeScript: strict mode, no `any` (enforced via ESLint).
- Commit messages: Conventional Commits (`feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `perf`, `ci`). Required for the PR title via `.github/workflows/pr-title.yml`.
- Formatting: Prettier for `src/`, `cargo fmt` for `src-tauri/`. `.editorconfig` and `.prettierrc` are checked in.
- `.typos.toml` runs in CI — if a real word triggers it, add it to the allowlist there rather than rewording.
- Don't commit a `pipeline:state` change without thinking about what the tray tooltip + capsule will show.

## Translated READMEs

`README_*.md` files (zh, ja, ko, es, fr, de, pt, ru, ar, hi, it, tr, vi, th, id, pl, nl) are kept in sync with `README.md`. If you change `README.md` substantively, flag it in the PR — translation updates can land separately.
