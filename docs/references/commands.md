# Commands

## Development

```bash
npm run tauri dev
```

Runs Vite on port 1420 and launches the Tauri shell.

## Production Build

```bash
npm run tauri build
```

Build output goes under `src-tauri/target/release/bundle/`.

To auto-open WebKit devtools for both windows:

```bash
npm run tauri dev -- --features devtools
```

## Frontend Checks

These mirror `.github/workflows/ci.yml`.

```bash
npx tsc --noEmit
npx eslint src/
npx prettier --check src/
npx vitest run
```

Targeted Vitest examples:

```bash
npx vitest run path/to/file
npx vitest -t "pattern"
```

## Rust Checks

```bash
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

Targeted Rust test example:

```bash
cargo test --manifest-path src-tauri/Cargo.toml test_parse_hotkey_ctrl_slash
```

## Cloud Backend Override

```bash
VITE_API_BASE_URL=https://my.example.com API_BASE_URL=https://my.example.com npm run tauri build
```

## Needs confirmation

- No separate docs-only validation command exists yet.
