# Troubleshooting

Short list of failure modes that surface in user reports.

## macOS: "I press the hotkey but nothing happens"

### Microphone is denied or restricted

OpenTypeless refuses to start the pipeline when macOS reports the Microphone authorization status as `denied` or `restricted` — letting `cpal` try to open the input device would fail silently and the macOS prompt is one-shot per install.

Fix:

1. System Settings → Privacy & Security → **Microphone**
2. Toggle OpenTypeless on (or add it if it isn't listed)
3. Re-press the hotkey

The main window also shows a red **Microphone denied** banner with a one-click deeplink to the right pane.

### Accessibility is granted but paste does nothing

macOS keys Accessibility (TCC) grants by bundle ID *plus* code signature. When a new build with a different signing identity replaces an older OpenTypeless.app — common for self-signed local builds, ad-hoc-signed dev builds, or fork-built bundles — the entry remains in System Settings but the OS silently denies it because the signature hash doesn't match. CGEventPost then drops every synthesised key without surfacing an error.

OpenTypeless detects this on the next paste attempt (`AXIsProcessTrusted()` returns false even though the entry exists) and surfaces the Accessibility banner. The user-visible symptom is "I granted permission but paste still doesn't work."

Fix:

1. System Settings → Privacy & Security → **Accessibility**
2. Select OpenTypeless, click the `-` to remove it
3. Trigger a dictation; the in-app banner re-prompts and macOS re-adds the entry
4. Toggle the new entry on
5. Dictate again

## macOS: "It keeps asking for my password to access the keychain"

API keys live in the login keychain (see [Storage → Credentials](../architecture/storage.md#credentials-os-credential-vault)). A keychain item's ACL matches on the app's **designated requirement**, so which builds it trusts depends on how they were signed:

- **Release builds** are signed with the "OpenTypeless Release" certificate, giving a requirement of `certificate leaf = H"…"`. That is stable across versions, so updating the app does *not* re-prompt.
- **Local builds** (`npm run tauri build` with no certificate) are ad-hoc signed, giving `cdhash H"…"` — a different identity on every rebuild. Each rebuild is a stranger to the previous build's keychain items and prompts once. This is expected while developing; click **Always Allow**.

Two things that look like this bug but aren't:

- Running `security find-generic-password -s com.opentypeless.app …` from a terminal prompts every time. `/usr/bin/security` is not on the item's ACL — that dialog says "**security** wants to use your confidential information", not "OpenTypeless". Read the app's own log instead: it reports `stt_key_len` at dictation time.
- Choosing **Allow** rather than **Always Allow** grants a single access. Reads are cached per session, so this costs at most one prompt per key per launch rather than one per dictation.

### Stopping the per-rebuild prompt while developing

Each ad-hoc build appends its own hash to the item's ACL once you click "Always Allow", so the prompts never stop — the next rebuild is a new stranger. Observed directly in `securityd`'s log: the ACL had accumulated the hashes of two earlier builds while a third, freshly built binary was the one asking.

Sign local builds with a stable self-signed certificate instead, and the ACL pins to the certificate rather than the hash:

1. Keychain Access → Certificate Assistant → **Create a Certificate…**
2. Name it (e.g. `OpenTypeless Dev`), Identity Type **Self Signed Root**, Certificate Type **Code Signing**.
3. Find it in **login**, open it, and set **Trust → Code Signing: Always Trust**.
4. Build with it:

```bash
APPLE_SIGNING_IDENTITY="OpenTypeless Dev" npm run tauri build
```

Every subsequent build signed with that certificate satisfies the same ACL entry, so you approve once. Delete existing entries first (Keychain Access, search `com.opentypeless.app`) so they get recreated against the certificate rather than an old hash.

If prompts genuinely repeat for a *released* build, the signing certificate has likely been rotated; every existing ACL entry then needs one "Always Allow" again.

Inspect what a bundle claims with:

```bash
codesign -d -r- /Applications/OpenTypeless.app
```

## macOS: "I granted Microphone but it never appeared again"

The macOS Microphone dialog is one-shot per install. If you dismissed or denied it, the only path forward is System Settings → Privacy & Security → Microphone. The onboarding Permissions step surfaces a deeplink button for this case.

## "Editing selected text by voice does nothing"

Check these in order:

1. **Is AI Polish on?** Settings → LLM. The LLM is what applies the spoken instruction, so with polish off the setting does nothing at all. The toggle is disabled in that state and says so, but a config saved by an older version can still have the feature on with polish off — turn polish on and the gate clears.
2. **Was the text still selected when you pressed the hotkey?** Clicking into another window to reach the capsule drops the selection in most apps. Use the hotkey, not the capsule, and keep focus where the text is.
3. **Did the capsule show the amber ring?** No ring means nothing was captured, and the dictation was inserted as ordinary text rather than treated as an instruction. In a browser or an Electron app the ring only appears once you release the hotkey, because the selection has to come through the clipboard there — see [Pipeline → Selected-Text Capture](../architecture/pipeline.md#selected-text-capture).
4. **Was what you said plausibly an instruction?** Dictating ordinary prose with something selected is treated as dictation, deliberately: the alternative is mangling a paragraph because you happened to have it highlighted.
5. **Password fields are never read**, by design.

Nothing is lost when an edit fails: the selection is left untouched and the error is surfaced. The raw transcript is never pasted over selected text.

## Non-macOS

The Permissions step is skipped on Linux and Windows — neither needs per-app Microphone or Accessibility grants. If recording fails, check that the default input device is selected in the OS sound settings.

Editing selected text by voice works on both, via a Ctrl+C capture rather than the macOS Accessibility read. The only difference is timing: the capsule's mode ring appears when the hotkey is released rather than at the start of recording.
