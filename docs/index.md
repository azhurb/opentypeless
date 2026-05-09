# OpenTypeless Documentation

Repository-local system of record for architecture, domain knowledge, decisions, and workflows.

`README.md` is the human-friendly product entrypoint. [`CLAUDE.md`](../CLAUDE.md) (and the `AGENTS.md` symlink) is the short agent entrypoint. Deeper knowledge lives here.

## Architecture

- [Overview](architecture/overview.md) — runtime shape, module boundaries, windows.
- [Pipeline](architecture/pipeline.md) — recording → STT → LLM → output.
- [Providers](architecture/providers.md) — STT and LLM provider traits and registry.
- [Frontend ↔ Backend](architecture/frontend-backend.md) — Tauri commands, events, two-window bundle.
- [Storage](architecture/storage.md) — `tauri-plugin-store` config and SQLite history/dictionary.

## Domain

- [Feature map](domain/features.md) — public features reconciled with repo evidence.
- [Voice input](domain/voice-input.md) — recording flow and prompt behavior.
- [Cloud Pro mode](domain/cloud-pro.md) — `cloud` providers and session token flow.
- [Glossary](domain/glossary.md) — project terms.

## References

- [Commands](references/commands.md) — canonical local dev / CI-equivalent commands.
- [Repository map](references/repo-map.md) — where important files live.
- [Conventions](references/conventions.md) — formatting, commits, translations.
- [Documentation maintenance](references/documentation-maintenance.md) — when and how to update docs.

## Decisions And Plans

- [Decision records](decisions/index.md)
- [Active plans](plans/active/README.md) · [Completed plans](plans/completed/README.md)
- [Generated references](generated/README.md) — schema/event/command dumps when generation tooling exists.

## The Single Rule

When a change touches behavior, terminology, architecture, providers, commands/events, storage, or workflows, update the matching doc in the same PR. Trigger list and writing rules: [`references/documentation-maintenance.md`](references/documentation-maintenance.md).

## Open Gaps

These are tracked here so future work can fill them rather than rediscover them:

- No automated docs lint, dead-link check, or freshness check.
- No generated reference for Tauri command signatures, event payloads, or DB schema (candidates listed in [`generated/README.md`](generated/README.md)).
- No source-of-truth doc for: audio capture (`src-tauri/src/audio/`), keyboard/clipboard output (`src-tauri/src/output/`), foreground-app detection (`src-tauri/src/app_detector/`), tray menu wiring, onboarding flow, hotkey parsing/modifier rules, deep-link/auth flow, scene packs, or the `/api/proxy/*` cloud contract.
