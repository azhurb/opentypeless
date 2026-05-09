# Documentation Maintenance

Docs change in the same PR as the code or behavior they describe. This page is the load-bearing rule that [`CLAUDE.md`](../../CLAUDE.md), [`CONTRIBUTING.md`](../../CONTRIBUTING.md), and [`docs/index.md`](../index.md) all link to.

## The Rule

If your change touches any of the triggers below, update the matching doc in the same PR. If it does not, add a one-liner to the PR description: `Docs: not affected.` That removes ambiguity from review.

## Triggers (and the doc each maps to)

| Triggered by | Update |
| --- | --- |
| New / removed / renamed STT or LLM provider, or factory match-arm changes | [`architecture/providers.md`](../architecture/providers.md), [`domain/features.md`](../domain/features.md) |
| Pipeline states, events, lifecycle, or invariants | [`architecture/pipeline.md`](../architecture/pipeline.md) (and event list in [`architecture/frontend-backend.md`](../architecture/frontend-backend.md)) |
| New / removed / renamed Tauri command or emitted event | [`architecture/frontend-backend.md`](../architecture/frontend-backend.md) |
| Storage schema, `AppConfig` shape or defaults, retention rules | [`architecture/storage.md`](../architecture/storage.md) |
| Hotkey, output mode, polishing, translation, selected-text, history, onboarding, or auth behavior | [`domain/voice-input.md`](../domain/voice-input.md), [`domain/features.md`](../domain/features.md) |
| `cloud` provider, base URL, session token, deep-link, or subscription check | [`domain/cloud-pro.md`](../domain/cloud-pro.md) |
| Public claims in `README.md` or on the OpenTypeless website features page | [`domain/features.md`](../domain/features.md) |
| Local dev commands, CI checks, or build/release steps | [`references/commands.md`](commands.md) (single source of truth — do not duplicate the list elsewhere) |
| New conventions or patterns future contributors must follow | [`references/conventions.md`](conventions.md) and a [decision record](../decisions/index.md) |
| New term or rename of an existing one | [`domain/glossary.md`](../domain/glossary.md) |

## Writing Rules

- Prefer short focused docs over a single manual.
- Link to code evidence (file paths, function names) so claims can be re-verified.
- Mark inferred behavior as such (`Inference:` line or sentence).
- Mark missing or uncertain content as `Needs confirmation`. Resolve those entries once an owner can confirm; do not let them accrete.
- Don't paste large README or code excerpts — link instead.

## Plans

- Active multi-step plans live in `docs/plans/active/`.
- Move to `docs/plans/completed/` if they are useful history; delete otherwise.

## Generated Docs

Generated content lives in `docs/generated/` and must declare:

- The source command or script.
- Whether manual edits are allowed.
- How to refresh.

Candidate generators (none built yet): Tauri command + signature dump, emitted-event list, `AppConfig` schema, SQLite schema.

## What Not To Do

- Do not duplicate command lists. They live in [`references/commands.md`](commands.md).
- Do not duplicate the agent entrypoint in `AGENTS.md` — it is a symlink to `CLAUDE.md`.
- Do not let `Needs confirmation` placeholders sit forever. Each one should name what would resolve it.

## Needs confirmation

- No automated docs lint, dead-link check, or freshness check exists.
- Owner / reviewer expectations for `Needs confirmation` resolution are not defined.
