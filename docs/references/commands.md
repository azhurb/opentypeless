# Commands

This page is the canonical command reference. `CLAUDE.md`, `CONTRIBUTING.md`, and CI workflows defer to it. CI mirror: `.github/workflows/ci.yml`.

## Development

```bash
npm run tauri dev
```

Runs Vite on port 1420 and launches the Tauri shell.

To auto-open WebKit devtools for both windows:

```bash
npm run tauri dev -- --features devtools
```

## Production Build

```bash
npm run tauri build
```

Output: `src-tauri/target/release/bundle/`.

Override the cloud backend at build time:

```bash
VITE_API_BASE_URL=https://my.example.com API_BASE_URL=https://my.example.com npm run tauri build
```

## Frontend Checks (mirrors `check-frontend` in CI)

```bash
npx tsc --noEmit
npx eslint src/
npx prettier --check src/
npx vitest run
```

Targeted Vitest:

```bash
npx vitest run path/to/file
npx vitest -t "pattern"
```

## Rust Checks (mirrors `check-rust` in CI on Windows / macOS / Linux)

```bash
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

Targeted test:

```bash
cargo test --manifest-path src-tauri/Cargo.toml test_parse_hotkey_ctrl_slash
```

## Audit (CI only, non-blocking)

`npm audit --audit-level=high` and `cargo audit --file src-tauri/Cargo.lock` run in the `audit` job and are `continue-on-error: true`.

## Needs confirmation

- No docs-only validation command (link checker, freshness check) exists yet. A simple grep-based check would catch the "Tauri command exists in `lib.rs` but no TS wrapper" class of bug.
