# 0001 - Repository-Local Documentation System

## Status

Accepted by repository change request.

## Context

The repo already had `README.md` and a long `CLAUDE.md`. The goal is to make repository knowledge the system of record while keeping the injected agent entrypoint short.

The approach is inspired by OpenAI's Harness engineering article: use a short agent-facing table of contents and keep deeper, versioned knowledge in a structured `docs/` tree.

## Decision

Use `docs/` as the canonical home for architecture, domain knowledge, decisions, workflows, references, and plans.

Keep:

- `README.md` as the human-friendly product entrypoint.
- `CLAUDE.md` as the concise agent entrypoint.
- Focused docs under `docs/` as the deeper system of record.

Initial top-level categories:

- `docs/architecture/`
- `docs/domain/`
- `docs/decisions/`
- `docs/plans/active/`
- `docs/plans/completed/`
- `docs/references/`
- `docs/generated/`

## Consequences

- New agents should start at `CLAUDE.md`, then follow links to `docs/index.md` and relevant focused docs.
- Architecture and workflow changes should update docs in the same PR.
- Long plans can be checked into `docs/plans/active/` and moved to `docs/plans/completed/` when done.
- Generated references can be added under `docs/generated/` when generation tooling exists.

## Needs confirmation

- Whether to add CI checks for dead links, stale generated docs, or missing docs updates.
- Whether completed plans should be kept forever or pruned on a schedule.
