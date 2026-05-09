# OpenTypeless Documentation

This folder is the repository-local system of record for architecture, domain knowledge, decisions, workflows, and plans.

`README.md` remains the human-friendly product entrypoint. `CLAUDE.md` remains the short agent entrypoint. Deeper knowledge belongs here.

## Start Here

- [Architecture overview](architecture/overview.md) - system map and main runtime boundaries.
- [Pipeline](architecture/pipeline.md) - microphone to STT to LLM to output flow.
- [Providers](architecture/providers.md) - STT and LLM provider abstraction.
- [Frontend/backend wiring](architecture/frontend-backend.md) - Tauri commands, events, windows, and state.
- [Storage](architecture/storage.md) - config, history, dictionary, and local persistence.
- [Feature map](domain/features.md) - user-facing features reconciled with repo evidence.
- [Voice input domain](domain/voice-input.md) - product behavior visible from existing code and README.
- [Glossary](domain/glossary.md) - project terms and abbreviations.
- [Commands](references/commands.md) - local development and CI-equivalent checks.
- [Repository map](references/repo-map.md) - where important files live.
- [Conventions](references/conventions.md) - formatting, commit, provider, and README translation rules.
- [Documentation maintenance](references/documentation-maintenance.md) - when docs must be updated.

## Decisions And Plans

- [Decision records](decisions/index.md) - durable project decisions.
- [Active plans](plans/active/README.md) - checked-in work plans in progress.
- [Completed plans](plans/completed/README.md) - historical plans worth preserving.
- [Generated references](generated/README.md) - generated docs such as schema references, when added.

## Documentation Rules

- Prefer short focused docs over one giant manual.
- Link to deeper docs instead of duplicating large sections.
- If knowledge comes from code structure rather than explicit docs, label it as an inference.
- If a section needs product, design, or maintainer judgment, mark it `Needs confirmation`.
- Update docs in the same PR as architecture, workflow, provider, storage, or user-facing behavior changes.

## Needs confirmation

- There is no explicit owner list for docs, plans, or decisions in the repo yet.
- There is no automated docs freshness check yet.
