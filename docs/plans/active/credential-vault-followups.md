# Credential Vault — Open Follow-ups

Written 2026-07-26, after the keychain migration landed
([`../completed/keychain-migration.md`](../completed/keychain-migration.md), #36). These are
the questions that surfaced during local testing and were deliberately left out of that PR.

## 1. Linux with no Secret Service provider — **fixed, wants a real-machine check**

`keyring`'s `linux-native-sync-persistent` is `linux-native` (kernel keyutils, session-scoped)
plus `sync-secret-service` (D-Bus, for persistence across reboots). On a normal desktop the
login keyring is unlocked by the session manager and this is invisible.

On a box with **no Secret Service provider at all** — a minimal WM (i3, sway without
`gnome-keyring`), a headless install, many containers — it is not merely degraded. Verified by
reading `keyring-3.6.3/src/keyutils_persistent.rs`:

- `set_secret` writes keyutils, then Secret Service, and **if the Secret Service write fails
  it reverts keyutils and returns the error**. There is no partial success to fall back on.
- `get_secret` reads keyutils first and falls back to Secret Service — fine in principle, but
  nothing was ever stored.
- `delete_credential` best-efforts keyutils, then requires Secret Service.

Consequences:

- **A fresh install cannot save a key at all.** `set_api_key` propagates the error, Settings
  shows "Could not save the key to the system credential store", and there is no other path —
  the app is unusable. Strictly worse than the pre-migration behavior, where the key went into
  `settings.json`.
- An *upgrade* on such a box degrades safely: the migration keeps the plaintext and retries
  (that is what the write-then-clear ordering buys). But the pipeline reads only the vault, so
  the retained plaintext is preserved and never used — the user keeps a key they cannot spend.

**Fixed** by `FallbackVault` + `FileVault`: when the store refuses a write the key goes to a
`0600` `credentials.json` and the Settings pane says it is unencrypted; a later working store
promotes it back out. Covered by unit tests against a failing vault.

Still wants confirmation **on a real vault-less Linux box** — the tests simulate the failure
with `MemoryVault::failing`, which is not the same as proving `keyring` fails the way the
source says it does there.

Note upstream has the same dependency and the same failing `set_credential`, but their
`AppConfig` still carries `stt_api_key` / `llm_api_key` and `resolve_config_secret` **prefers
the config value over the vault** — so a hand-edited `settings.json` still works for them. We
removed those fields, so we do not have that escape hatch.

Options, roughly in order of preference:

1. **Detect an unavailable vault and fall back to `settings.json`, loudly.** Keeps the app
   usable, and the Settings pane must say plainly that the key is stored unencrypted. This is
   a fallback for a broken environment, not a user preference.
2. Ship a `linux-native`-only mode (keyutils, no D-Bus) and accept re-entry after reboot.
3. Do nothing until someone reports it.

Deliberately **not** proposed: a user-facing "disable keychain" toggle. It is a security
setting most users cannot evaluate, and it would re-legitimize plaintext storage on platforms
where the vault works fine.

## 2. Code signing — **now blocks the macOS Keychain**

Release builds are signed with a self-signed "OpenTypeless Release" certificate. The
designated requirement (`certificate leaf = H"…"`) *is* stable across versions, so the item
**ACL** keeps matching — but that is not the check that prompts.

The XARA **partition list** is, and its entries are keyed by `teamid:` only for apps with an
Apple Developer ID. With no Team ID, macOS keys them by `cdhash:`, so every release is a
stranger. Measured with two pipeline-signed builds sharing one certificate:

```
ACL partition mismatch: client cdhash:9fd54284…  ACL (cdhash:d1f9d146…)
```

That is why macOS now uses the file store (see
[`architecture/storage.md`](../../architecture/storage.md#macos-deliberately-does-not-use-the-keychain)).

An Apple Developer account ($99/yr) would buy three things at once:

- Stable `teamid:` partitions → the macOS Keychain becomes usable, no prompts across updates.
- Notarization → no Gatekeeper "unidentified developer" wall on download.
- Parity with upstream, who pass `APPLE_CERTIFICATE` + `APPLE_SIGNING_IDENTITY` +
  `APPLE_TEAM_ID` and notarize.

A spend decision, not a code one. When it happens, flip `default_store` to use
`SystemCredentialVault` on macOS and `FILE_STORE_IS_THE_DEFAULT` to `false`.

## 3. Release workflow accepts a non-tag ref

`release.yml` computes `VERSION="${{ inputs.tag || github.ref_name }}"` with no validation, so
dispatching it on a *branch* produced a release versioned `0.1.14` out of nowhere and shipped a
bundle whose About page showed that. A guard rejecting anything that is not `vX.Y.Z` would have
failed the run loudly instead.

## 4. Smaller items

- `MemoryVault` lives in the shipped binary rather than behind `#[cfg(test)]`, so integration
  tests and other modules' test modules can use it. Nothing constructs one in the app. If that
  ever stops being true, gate it.
- Secrets sit in `CachingVault` for the session in plain `String`s. They were already in
  memory during any request; zeroizing on drop was judged not worth the dependency, but it is
  the obvious next step if the threat model tightens.
