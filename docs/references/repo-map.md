# Repository Map

Where important files live. For module responsibilities see [Architecture overview](../architecture/overview.md).

## Product And Community Docs

- `README.md` — human-friendly product overview, setup, screenshots, FAQ.
- `README_*.md` — translated READMEs (rules in [Conventions](conventions.md#translated-readmes)).
- `CONTRIBUTING.md` — contribution guidance; defers to [`commands.md`](commands.md) for checks.
- `SECURITY.md` — vulnerability reporting.
- `VISION.md` — project principles and direction.
- `CLAUDE.md` — short agent entrypoint. `AGENTS.md` is a symlink to it.
- `docs/` — repository-local knowledge base ([`docs/index.md`](../index.md) is the map).

## Frontend

- `src/App.tsx` - main/capsule app switch and initial data load.
- `src/components/` - UI components.
- `src/components/Capsule/` - floating widget UI.
- `src/components/Settings/` - settings panes and provider configuration UI.
- `src/hooks/` - React hooks for theme, Tauri events, recording, capsule resize.
- `src/lib/tauri.ts` - TypeScript wrappers for Rust Tauri commands.
- `src/stores/appStore.ts` - app state and config types.
- `src/i18n/` - localization setup and strings.

## Backend

- `src-tauri/src/lib.rs` - Tauri setup, commands, tray, hotkey handling, app bootstrap.
- `src-tauri/src/main.rs` - app entrypoint.
- `src-tauri/src/pipeline.rs` - recording, STT, LLM, output orchestration.
- `src-tauri/src/audio/` - microphone capture (`capture.rs`) and macOS permission FFI (`permission.rs` + ObjC shim `mic_permission.m`, compiled by `build.rs`).
- `src-tauri/src/stt/` - STT provider abstraction and implementations.
- `src-tauri/src/llm/` - LLM abstraction, providers, prompt builder.
- `src-tauri/src/output/` - keyboard and clipboard output implementations.
- `src-tauri/src/storage/` - config, history, dictionary storage.
- `src-tauri/src/app_detector/` - foreground app detection and app type classification.
- `src-tauri/migrations/` - SQLite schema files.
- `src-tauri/tauri.conf.json` - Tauri app/window/bundle config.

## Tests

- `src/**/*.test.ts` and `src/**/*.test.tsx` - frontend unit/component tests.
- Rust unit tests currently live beside backend modules.

## Generated And Build Output

- `dist/` - frontend build output, ignored when generated locally.
- `src-tauri/target/` - Rust/Tauri build output, ignored when generated locally.
