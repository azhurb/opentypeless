# Selected-Text Editing In Accessibility-Blind Apps

Opened 2026-08-15, when the Cmd+C fallback was removed. Records what coverage was
given up, why narrowing the old approach was rejected, and the options for
winning the lost targets back.

## What changed

Edit-selected-text-by-voice now requires the macOS Accessibility preflight in
`pipeline::start`. The `Cmd`/`Ctrl+C` capture that used to run in `stop()`
whenever the preflight came up empty is gone. Mechanism and reasoning:
[Pipeline → Selected-Text Capture](../../architecture/pipeline.md#selected-text-capture).

## What it cost

| Target | Before | Now |
| --- | --- | --- |
| macOS native apps (`AXTextField`, `AXTextArea`, …) | Works, with ring | Works, with ring |
| Browser web content (Chrome, Safari) | Worked via Cmd+C, no ring | Not supported |
| Electron apps (Cursor, VS Code, Slack) | Worked via Cmd+C, no ring | Not supported |
| Windows, Linux | Worked via Ctrl+C, no ring | Not supported; toggle disabled |

## Why the fallback was removed rather than fixed

The fallback read "the clipboard changed after we sent Cmd+C" as "the user
selected text to edit". That inference does not hold:

- VS Code and its forks ship `editor.emptySelectionClipboard: true`, so Cmd+C
  with **no selection** copies the whole current line. JetBrains IDEs match. Both
  are Electron or otherwise AX-blind, so they hit the fallback on every dictation.
- A wrong answer switched the request to `SELECTED_TEXT_PROMPT`, which permits
  rewriting, reordering and tone changes — the inverse of the dictation prompt's
  "minimal edits, do not rephrase".
- The mode ring is suppressed for a selection discovered after the user has
  spoken (an early warning that arrives late is not a warning), so there was no
  signal that a different mode had engaged.

Narrowing was considered and rejected:

- **Sentinel-clear the clipboard before the copy.** Detects "the copy did
  nothing" reliably, which the old `selected == backup` compare got wrong in both
  directions. Does not detect "the app copied something the user did not select",
  which is the actual failure.
- **Reject whole-line artifacts** (single line, trailing newline). A heuristic
  that a genuine one-line selection defeats, and that misses multi-line cases.
- **Lean harder on the prompt's plain-dictation fallback rule.** Already present
  as rule 5 of `SELECTED_TEXT_PROMPT`; the small, fast models this app targets
  apply it unreliably, which is how the bug was noticed.

Losing an edit is recoverable. Silently rewriting text the user did not select is
not, so the ambiguous signal was removed rather than tuned.

## Options for winning the targets back

Roughly cheapest first. None is scheduled.

### 1. Explicit edit hotkey

A second hotkey that means "this dictation edits my selection". Edit mode becomes
a user decision instead of an inference, so the Cmd+C capture becomes sound again
— the user has asserted there is a selection, and copying it is no longer a
guess. Works everywhere, including Windows and Linux.

Cost: a second global hotkey registration and its conflict handling, a Settings
field, onboarding copy, and a capsule state that distinguishes the two modes.

Note the removed `output::copy_selection` / `clipboard::invoke_copy` machinery is
what this would need back; it is in git history at the commit that removed it,
including the main-thread dispatch and the layout-independent keycode handling
that took two bugs to get right.

### 2. Read the Edit ▸ Copy menu item's enabled state

macOS apps expose their menu bar through Accessibility even when their content is
opaque to it, because the menu bar is native AppKit. An `AXMenuItem` for Copy that
reports disabled is a strong "nothing is selected" signal, and one that reports
enabled is a decent "something is". Read at record start, this restores the ring
for Electron and browser targets and needs no keystroke at all.

Unverified, and the risks are real: apps that never validate their menu items
would report enabled always, Electron's menu implementation may not update
`AXEnabled` promptly, and traversing to the item costs AX round-trips inside the
500 ms preflight budget. Worth a spike against Cursor, Chrome and Slack before
committing.

### 3. Per-platform selection APIs

Windows UI Automation (`TextPattern.GetSelection`) and AT-SPI on Linux are the
structural equivalents of the macOS AX read. This is the only option that makes
the feature work off macOS without a keystroke.

Cost: two more platform backends behind `correction::FocusedField`, each with its
own permission and reliability story. Largest of the three by a wide margin.

## If the decision needs revisiting

The user-visible symptom that motivated this was: an ordinary dictation in Cursor
came back as a rewrite of a line the user had not selected. Any replacement
design has to make that impossible, not merely unlikely — the failure is silent
and destroys text the user already had.
