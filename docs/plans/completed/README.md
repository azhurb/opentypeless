# Completed Plans

Plans worth keeping as project history. Move a plan here from `../active/` once the work lands.

## Current Completed Plans

- [Keychain migration](keychain-migration.md) — API keys out of plaintext `settings.json` and into the OS credential vault, write-only from the webview. Landed 2026-07-26 (#36); kept for the write-only-vs-encrypted-at-rest analysis and the migration-ordering hazards.
- [Provider retry + pooled HTTP client](provider-retry.md) — retry with backoff where it is safe (streaming `connect`, the Whisper-compatible upload, the LLM request head — never mid-stream), plus one pooled `reqwest::Client`. Landed 2026-07-26; kept for the retry-safety analysis and the recorded follow-ups.

## Needs confirmation

- Retention policy for completed plans is not defined.
