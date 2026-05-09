# CLAUDE.md

Short agent entrypoint for OpenTypeless. Keep this file stable: project summary, where the docs live, and the small set of rules an agent must apply on every change. Anything else belongs under `docs/`.

## Project

OpenTypeless is a Tauri 2 desktop app (Windows/macOS/Linux) for AI voice input: hold a hotkey, speak, and the app transcribes via an STT provider, polishes via an LLM, and types the result into the foreground app.

- Frontend: React 19 + TypeScript + Tailwind 4 + Vite, Zustand, i18next.
- Backend: Rust 2021 + Tokio + Tauri 2. Audio: `cpal`. Output: `enigo`, `arboard`. Storage: `tauri-plugin-store` + `rusqlite`. HTTP/streaming: `reqwest`, `tokio-tungstenite`.

## Where Knowledge Lives

- Start at [`docs/index.md`](docs/index.md) — the canonical map.
- Local commands: [`docs/references/commands.md`](docs/references/commands.md).
- Conventions: [`docs/references/conventions.md`](docs/references/conventions.md).
- When and how to update docs: [`docs/references/documentation-maintenance.md`](docs/references/documentation-maintenance.md).

`README.md` is the human-facing product entrypoint and should not be treated as architecture documentation.

## Rules For Every Change

1. **Update docs in the same PR** when a change touches: architecture, pipeline states/events, providers, Tauri commands, storage schema or config fields, hotkey/output behavior, onboarding/auth, build or CI commands, or any user-facing behavior described in `README.md` / `docs/`. The full trigger list is in [`docs/references/documentation-maintenance.md`](docs/references/documentation-maintenance.md).
2. **Mark inference and uncertainty.** If a statement comes from reading code rather than explicit documentation, say so. Use `Needs confirmation` for sections that require maintainer judgment.
3. **Do not duplicate.** Link to the existing doc instead of restating it. CLAUDE.md, `docs/index.md`, and `docs/references/commands.md` each have one job; keep them from drifting apart.
4. **Keep CLAUDE.md short.** New architectural detail goes under `docs/`, not here.
