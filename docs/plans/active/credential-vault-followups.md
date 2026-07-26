# Credential Vault — Open Follow-ups

Written 2026-07-26, after the keychain migration landed
([`../completed/keychain-migration.md`](../completed/keychain-migration.md), #36). These are
the questions that surfaced during local testing and were deliberately left out of that PR.

## 1. Linux with no Secret Service provider — **the real gap**

`keyring`'s `linux-native-sync-persistent` is `linux-native` (kernel keyutils, session-scoped)
plus `sync-secret-service` (D-Bus, for persistence across reboots). On a normal desktop the
login keyring is unlocked by the session manager and this is invisible.

The open question is a box with **no Secret Service provider at all** — a minimal WM (i3,
sway without `gnome-keyring`), a headless install, some containers. **Needs confirmation** on
a real such machine, but the likely behavior is that vault writes fail outright, and if so:

- **A fresh install cannot save a key at all.** `set_api_key` propagates the error, the
  Settings pane shows "Could not save the key to the system credential store", and there is no
  other path — the app is unusable. This is worse than the pre-migration behavior, where the
  key simply went into `settings.json`.
- An *upgrade* on such a box degrades safely: the migration keeps the plaintext and retries
  (that is what the write-then-clear ordering buys). But the pipeline reads only the vault, so
  the retained plaintext is preserved and never used — the user keeps a key they cannot spend.

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

## 2. Code signing

Release builds are signed with a self-signed "OpenTypeless Release" certificate, so the
designated requirement is `certificate leaf = H"…"` — stable across versions, and Keychain
ACLs keep matching after an update. That is sufficient for the credential path.

Two things it does not buy, both of which upstream has (they use `APPLE_CERTIFICATE` +
`APPLE_SIGNING_IDENTITY` + `APPLE_TEAM_ID` and notarize):

- Gatekeeper still treats the download as unidentified — users get the right-click-Open dance.
- Rotating the certificate invalidates every existing Keychain ACL, so users would see one
  prompt after that release. Worth remembering before regenerating it.

Moving to Developer ID + notarization needs an Apple Developer account ($99/yr) — a spend
decision, not a code one.

## 3. Smaller items

- `MemoryVault` lives in the shipped binary rather than behind `#[cfg(test)]`, so integration
  tests and other modules' test modules can use it. Nothing constructs one in the app. If that
  ever stops being true, gate it.
- Secrets sit in `CachingVault` for the session in plain `String`s. They were already in
  memory during any request; zeroizing on drop was judged not worth the dependency, but it is
  the obvious next step if the threat model tightens.
