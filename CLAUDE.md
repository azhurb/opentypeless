# CLAUDE.md

This file is the short agent entrypoint for OpenTypeless. Keep it concise; deeper repository knowledge belongs in `docs/`.

## Project

OpenTypeless is a Tauri 2 desktop app (Windows/macOS/Linux) for AI voice input: hold a hotkey, speak, and the app transcribes via an STT provider, polishes via an LLM, and types the result into the foreground app.

- Frontend: React 19 + TypeScript + Tailwind 4 + Vite, Zustand for state, i18next for translations.
- Backend: Rust 2021, Tokio, `cpal` for audio capture, `enigo` for keyboard simulation, `arboard` for clipboard, `rusqlite` (bundled) for SQLite, `reqwest` for HTTP, `tokio-tungstenite` for streaming STT.

## Documentation Map

Start with `docs/index.md`.

- Architecture overview: `docs/architecture/overview.md`
- Pipeline: `docs/architecture/pipeline.md`
- Providers: `docs/architecture/providers.md`
- Frontend/backend wiring: `docs/architecture/frontend-backend.md`
- Storage: `docs/architecture/storage.md`
- Feature map: `docs/domain/features.md`
- Voice input domain: `docs/domain/voice-input.md`
- Cloud Pro mode: `docs/domain/cloud-pro.md`
- Glossary: `docs/domain/glossary.md`
- Commands: `docs/references/commands.md`
- Repo map: `docs/references/repo-map.md`
- Conventions: `docs/references/conventions.md`
- Documentation maintenance: `docs/references/documentation-maintenance.md`
- Decisions: `docs/decisions/index.md`
- Plans: `docs/plans/active/README.md`

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

Use `docs/architecture/overview.md` as the map. Do not let this file become the architecture manual.

## Conventions

- Follow `docs/references/conventions.md`.
- If a change affects architecture, providers, pipeline behavior, storage, commands, events, or user-facing behavior, update docs in the same PR.
- If behavior is inferred from code rather than explicitly documented, say so.
- If unsure, mark the relevant doc section `Needs confirmation`.
