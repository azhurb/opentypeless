# Selected-Text Editing Where Accessibility Cannot Reach

Opened 2026-08-15 when the Cmd+C fallback was removed. **Substantially rewritten
2026-08-24**: the premise it was built on turned out to be wrong, and most of the
coverage it mourned was never actually lost to the platform.

## What the original version got wrong

It recorded that Accessibility cannot see browser web content or Electron apps,
and listed three ways to win those targets back. Measured on macOS 26
(Darwin 25.5), that framing was incorrect in both directions:

- **The system-wide element answers nothing at all.**
  `AXUIElementCopyAttributeValue(systemWide, kAXFocusedUIElementAttribute)`
  returns `kAXErrorCannotComplete` (-25204) in 0 ms — a refusal, not a timeout —
  for every app tried, TextEdit included. Native apps only *appeared* to work
  because the Cmd+C fallback was covering for a read that had already failed.
  Removing the fallback in 0.8.0 exposed a breakage that had been there all
  along, and it was misreported as a deliberate narrowing.
- **Chromium is not blind.** Asking the frontmost application's own element
  (`AXUIElementCreateApplication(pid)`) reads a Gmail draft in Chrome in ~31 ms:
  `role=AXTextArea`, `AXSelectedText="Test gmail selection."`. Browser text
  fields work. So does anything served through them, including Slack on
  `app.slack.com`.
- **`AXManualAccessibility` is neither needed nor supported.** Chrome answered
  `kAXErrorAttributeUnsupported` (-25205) when we tried to set it, and read the
  selection fine without it. The "enable Chromium's a11y tree" route in the
  original version was solving a problem that does not exist.

The fix was `focus_root` in `correction::ax_macos`. See
[Pipeline → Selected-Text Capture](../../architecture/pipeline.md#selected-text-capture).

## What is actually still missing

| Target | Status | Why |
| --- | --- | --- |
| Native macOS apps | Works | `AXSelectedText` off the frontmost app element |
| Browser text fields (Chrome, Safari), incl. `app.slack.com` | Works | Chromium exposes the input's selection |
| Slack desktop app | **Unverified** | Electron, so expected to behave like Chrome, but never measured — not installed on the test machine |
| VS Code, Cursor | Does not work | Monaco answers `kAXErrorNoValue` (-25212) promptly: it publishes no focused element until it detects a screen reader |
| Static web page text (not a form field) | **Unverified** | Chromium exposes this as `AXSelectedTextMarkerRange`, not `AXSelectedText`, so it likely needs separate handling |
| Windows, Linux | Does not work | `correction::ax_stub` has no implementation; the Settings toggle is disabled there |

## Open work, cheapest first

### 1. Measure the two unverified rows

Both are a probe away and neither needs new code. The Slack desktop app matters
because it is the app most often asked about; the marker-range case matters
because "select a paragraph on a web page and reword it" is a plausible thing to
want, and it is the one browser case we know `AXSelectedText` will not cover.

### 2. Static web text via `AXSelectedTextMarkerRange`

If the row above confirms it, reading a page-text selection means the
parameterized marker-range attributes rather than the plain string one. Larger
than it sounds: marker ranges are opaque and need
`AXStringForTextMarkerRange` to resolve, and writing the result back is a
different problem again, since a web page selection is usually not editable.
Read-only uses (translate, summarize into the clipboard) may be the only sane
scope.

### 3. Monaco-based editors

VS Code and Cursor gate accessibility behind screen-reader detection.
`editor.accessibilitySupport: "on"` makes the editor expose its content, but
that is a user-side setting we cannot set for them and it changes the editor's
own behaviour. The honest options are to document it, or to detect the editor
and tell the user why the ring never appears there.

### 4. Non-macOS

Windows UI Automation (`TextPattern.GetSelection`) and AT-SPI on Linux are the
structural equivalents, behind `correction::FocusedField`. Unchanged from the
original version, and still the largest item here.

## What is no longer on the list

- **An explicit edit hotkey.** Proposed when the alternative looked like
  clipboard guessing. With the AX read working across native and browser
  targets, a second hotkey buys only Monaco editors and non-macOS, at the cost
  of a permanent second global shortcut. Not worth it on that basis alone.
- **Reading the Edit ▸ Copy menu item's enabled state.** Same reasoning: it was
  a way to infer "something is selected" without AX, and AX now answers.
- **Reinstating the Cmd+C fallback.** Its own defect is unchanged and unrelated
  to any of this: it read any clipboard change as a selection, and VS Code
  copies the current line when nothing is selected. It should stay removed.

## The rule this cost us

The original version asserted a platform limitation from reading code comments
rather than from measuring. It then shipped that assertion in release notes as a
deliberate trade-off. Before recording that a platform cannot do something,
measure it — a probe against a live app took minutes and reversed the
conclusion.
