# Conventions

## TypeScript

- Strict mode is enabled.
- Avoid `any`; ESLint enforces this.
- Use Prettier for files under `src/`.

## Rust

- Use Rust 2021.
- Run `cargo fmt` and `cargo clippy` for backend changes.

## Commits And Pull Requests

- Use Conventional Commits: `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `perf`, `ci`.
- PR titles are checked by `.github/workflows/pr-title.yml`.

## Providers

When adding a provider, update Rust provider creation, TypeScript provider unions, connection tests, benchmark match arms, Settings UI, and docs.

## Pipeline State

Do not change `pipeline:state` behavior without considering tray tooltip and capsule UI behavior.

## Translated READMEs

`README_*.md` files are translations of `README.md`.

If `README.md` changes substantively, flag it in the PR. Translation updates can land separately.

## Typos

`.typos.toml` runs in CI. If a real word triggers it, add it to the allowlist rather than awkwardly rewording.

## Needs confirmation

- The exact threshold for a "substantive" README change is not defined.
