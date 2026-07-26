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

## CI Does Not Run Automatically On This Fork

**Run the checks below locally before opening a PR. A PR with no red check has not been
verified — it means nothing ran.**

GitHub disables workflows on forks by default. The enable action exists only as a banner in
the repository's **Actions tab** ("Workflows aren't being run on this forked repository" →
*"I understand my workflows, go ahead and enable them"*); it is deliberately absent from
Settings → Actions, and there is no REST API for it. Note that
`GET /repos/{owner}/{repo}/actions/permissions` returns `"enabled": true` regardless — that
field is the permissions *policy*, not the fork's activation state, so it is not a way to
check this.

Until that banner is accepted, `on: push` and `on: pull_request` never fire. Every run in
this repository's history is `workflow_dispatch`, and `ci.yml` had **0 runs** up to
2026-07-26 — which is why 14 `clippy -D warnings` errors and both formatter failures reached
`v0.5.0` unnoticed. It is also the same root cause as the release tag-push trigger stalling
(see [Cutting a release](#cutting-a-release), step 4).

Both workflows accept a manual trigger, so CI can be run on demand either way:

```bash
gh workflow run ci.yml                      # current default branch
gh workflow run ci.yml --ref <branch>       # a specific branch
gh run list --workflow ci.yml --limit 5     # check results
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

## Releases

Releases are tag-driven. `.github/workflows/release.yml` triggers on tags matching `v*` and on manual `workflow_dispatch`.

### Cutting a release

1. Pick the next version above the highest existing `vX.Y.Z` tag (`git tag --sort=-version:refname | head -1`).
2. **Fold the changelog before tagging.** Open a small PR that renames the `[Unreleased]` section in `CHANGELOG.md` to `[X.Y.Z] - YYYY-MM-DD` and merge it. Without this step, `git checkout vX.Y.Z` shows the release's changes under `[Unreleased]` even though they have shipped — the tag points at a commit where the file disagrees with reality.
3. Tag the fold's merge commit on `main` and push the tag:

   ```bash
   git tag v0.1.25 <merge-commit-sha>
   git push origin v0.1.25
   ```

4. **If the release workflow doesn't trigger automatically** (tag-push triggers can stall on this fork), kick it manually:

   ```bash
   gh workflow run release.yml --field tag=v0.1.25
   ```

5. The `Release` workflow runs four parallel builds: Windows (`x86_64-pc-windows-msvc`), macOS arm64 (`aarch64-apple-darwin`), macOS x86_64 (`x86_64-apple-darwin`), Linux (`x86_64-unknown-linux-gnu`).
6. CI strips the leading `v` and writes the version into `package.json`, `src-tauri/tauri.conf.json`, and `src-tauri/Cargo.toml` *during the build only* — these files stay at `0.1.0` in git. **Do not commit version bumps.**
7. `tauri-apps/tauri-action@v0` uploads the artifacts to a **draft** GitHub Release with a stub body. Replace the body with proper release notes (sections from the `CHANGELOG.md` entry plus a Downloads section that includes the macOS Gatekeeper `xattr -dr com.apple.quarantine` workaround — the build is signed but not notarized, so Sequoia / Tahoe block first launch). Use a prior release as a style reference. Smoke-test the artifacts, then publish from the Releases page (default to non-prerelease for visibility).

### Re-running a build for an existing tag

Run the workflow via `workflow_dispatch` and pass the tag name (e.g. `v0.1.25`). This rebuilds without retagging.

## Needs confirmation

- No docs-only validation command (link checker, freshness check) exists yet. A simple grep-based check would catch the "Tauri command exists in `lib.rs` but no TS wrapper" class of bug.
