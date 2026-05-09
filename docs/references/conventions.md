# Conventions

## TypeScript

- Strict mode is enabled.
- No `any` (ESLint enforces).
- Prettier formats files under `src/`.

## Rust

- Rust 2021.
- Run `cargo fmt` and `cargo clippy -- -D warnings` for backend changes.

## Commits And Pull Requests

- [Conventional Commits](https://www.conventionalcommits.org/): `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `perf`, `ci`.
- PR titles are checked by `.github/workflows/pr-title.yml`.

## Providers

The full add-a-provider checklist lives in [Providers → Adding A Provider](../architecture/providers.md#adding-a-provider). Do not duplicate it here.

## Pipeline State

`pipeline:state` is consumed by both the tray tooltip and the capsule UI. Consider both when changing state semantics. See [Pipeline → States](../architecture/pipeline.md#states).

## Translated READMEs

`README_*.md` are translations of `README.md`. If `README.md` changes substantively, flag it in the PR. Translations can land in follow-up PRs.

"Substantive" is intentionally informal: anything that changes documented behavior, screenshots, supported providers, or installation steps qualifies. Pure typo fixes do not.

## Typos

`.typos.toml` runs in CI. Add real words that trip it to the allowlist rather than awkwardly rewording.

## Documentation

Behavior or workflow change → matching doc update in the same PR. Triggers and writing rules: [Documentation maintenance](documentation-maintenance.md).
