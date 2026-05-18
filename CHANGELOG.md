# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/), and this project adheres to [Semantic Versioning](https://semver.org/).

## Fork

This repository is a fork of [Tover0314/opentypeless](https://github.com/tover0314-w/opentypeless). The entry for `0.1.0` describes the upstream baseline; `0.2.0` is the fork's first release, marking the BYOK-only direction and the changes listed below.

## [Unreleased]

### Fixed
- First-few-words clipping at dictation start, especially noticeable on short utterances and worse under variable system load. `PipelineHandle::start()` used to run config load, foreground-app detection (three sequential `osascript "tell application System Events"` shell-outs, ~150–450 ms cold), STT WebSocket connect (~100–500 ms for streaming providers), and `cpal` stream open (~50–300 ms) *before* any audio sample was captured — so the first ~300 ms–1.2 s of speech after key-down was discarded. Two changes close the gap: `AudioCaptureHandle::start()` now opens the cpal stream first, and the audio mpsc channel (200 chunks × 20 ms ≈ 4 s) absorbs samples while the slow setup runs in the background; once STT connects, the forwarder task flushes the pre-buffer. macOS foreground-app detection is rewritten to use `NSWorkspace.frontmostApplication` (via the Objective-C runtime) plus an AX `AXFocusedWindow → AXTitle` read, replacing the three osascript spawns with a single in-process call (<5 ms). `recording_start` now stamps when capture really begins so the `pipeline:timing.recording_ms` metric stops under-reporting by the dead-window amount. See [`docs/plans/active/dictation-startup-latency.md`](docs/plans/active/dictation-startup-latency.md) for the timing breakdown and deferred follow-ups.

## [0.3.1] - 2026-05-18

### Fixed
- Intermittent "only V typed instead of paste" bug on macOS, especially in browser text inputs and other Chromium/Electron surfaces. macOS Cmd+V is now synthesised by building the V key-down and key-up CGEvents directly via `core-graphics` and stamping `kCGEventFlagMaskCommand` on each event with `CGEventSetFlags`, rather than going through `enigo` 0.2.x which posted a separate Cmd `flagsChanged` event and relied on `CombinedSessionState` to propagate the modifier onto the V event. Under load that propagation raced the V event's creation and the receiving app saw a plain V keystroke, typing a literal "v" instead of pasting. Windows/Linux still use `enigo` Ctrl+V; the race was macOS-specific.

## [0.3.0] - 2026-05-17

### Added
- Multi-language STT: Settings → Language is now a chip picker over a *set* of expected languages. Empty = auto-detect; one = pin at the wire; two-plus = auto-detect with the polish prompt biased toward your set. Detected language shows as a per-row badge in History and triggers a rate-limited toast when you dictate in a language you haven't configured. A one-shot load-time migration converts the previous `stt_language: "multi"` / `"en"` setting into the new array shape.
- Per-target paste chunking for terminal-hosted CLIs (Claude CLI, Codex CLI, Gemini CLI). When the foreground app is a recognised terminal emulator or IDE terminal panel (Terminal.app, iTerm2, Warp, Ghostty, Kitty, Alacritty, Hyper, WezTerm, VS Code, Cursor, Windsurf, JetBrains family) and the window title matches a known CLI name, the paste is split into chunks with brief delays so the CLI's input buffer doesn't drop characters.
- Onboarding (macOS) now includes an explicit Permissions step that asks for Microphone and Accessibility up front, so users see the system prompts while they're paying attention instead of mid-dictation.
- Pre-flight macOS Accessibility check before paste. When the grant is missing the pipeline emits an `ACCESSIBILITY_REQUIRED` error code instead of silently dropping every synthesised keystroke; the main window shows an Accessibility banner with a Grant button, and the capsule surfaces a clear message.
- Pre-flight macOS Microphone check before recording. When the system status is `denied` / `restricted` the hotkey no longer starts a doomed pipeline run; a red banner in the main window points to System Settings → Privacy & Security → Microphone.
- New Tauri commands `check_microphone_permission` and `request_microphone_permission` (macOS, no-ops elsewhere) wrapping `AVCaptureDevice.authorizationStatus` / `requestAccess` via a small ObjC shim.
- Troubleshooting reference at `docs/references/troubleshooting.md` covering the macOS signature-mismatch case and the one-shot Microphone dialog.

### Changed
- Output is now exclusively clipboard-paste with the user's prior clipboard snapshotted and restored. Cmd+V (Ctrl+V on Windows/Linux) is synthesised directly via `CGEventPost`; the prior osascript / System Events round-trip is gone, so users only need to grant macOS **Accessibility** (a single grant covers both paste and the correction watcher) — no separate Automation permission is required.
- Foreground app detection on macOS now also captures the bundle identifier, used to drive per-target paste behavior.
- Settings broadcast: settings edits now reach every webview immediately via a `config:changed` event. The floating capsule reacts to changes like "Hide capsule when idle" without an app restart.
- Polish prompt is language-aware. It receives both the STT-detected language and the user's configured language set, so the polished output respects what you actually spoke.

### Fixed
- Paste-time crash on macOS Sequoia / Tahoe. `enigo`'s `CGEventSource::new()` internally calls `TSMGetInputSourceProperty`, which the OS asserts must be on the main thread; running it on a Tokio worker (introduced when paste moved to direct `CGEventPost` in PR #7) caused intermittent `SIGTRAP` aborts under input-source flux (right after granting Accessibility, switching apps, etc.). The Cmd+V synth now dispatches to Tauri's main thread via `AppHandle::run_on_main_thread`; the clipboard write stays on the worker thread (arboard is thread-safe).
- Hidden-window-during-onboarding. The "should I surface the main window at launch" predicate was hardcoded to `stt_api_key.is_empty()`, so users whose STT key was already configured but who needed to re-run onboarding (e.g. after a `tccutil`-driven permissions reset) landed on a tray-only launch with no visible UI. Predicate now also considers `onboarding_completed` and is extracted as `should_show_window_on_launch` with truth-table tests.
- Onboarding wiping existing API keys. `App.tsx` previously skipped `getConfig()` entirely when `onboarding_completed` was false, so the Zustand store stayed on `defaultConfig` (empty keys) while the user moved through the flow; the final-step save then wrote those empties over the still-on-disk values. Config is now loaded unconditionally so onboarding pre-populates from disk and re-running the flow is idempotent.
- "Learn From Corrections" toggle no longer shows on non-macOS where it would be a no-op.
- Output normalises CR (`\r`) and Unicode line separators to LF before paste, fixing odd line breaks in pasted multi-line text.
- Release builds for Linux and Windows now compile again — `build.rs` cfg-gates the macOS-only `cc::Build` call so non-Mac targets don't fail at compile time.

### Removed
- "Output Mode" setting in Settings → General and the associated macOS Accessibility permission card.
- Streaming-as-you-type output: LLM polish output now lands as a single paste once polish completes. The capsule still renders the live polish indicator from `llm:chunk` events.
- Single-string `stt_language` config field; replaced with `stt_languages: Vec<String>` (auto-migrated on first launch).

## [0.2.0] - 2026-05-10

First fork release. Cuts cloud / account / subscription / telemetry surfaces and ships substantive UX and reliability work on top of upstream `0.1.0`.

### Added
- Streaming keyboard output — LLM tokens are typed as they arrive instead of after the full response
- Live mic volume drives the capsule waveform bars during recording
- Indeterminate progress bar in the capsule replaces the "Transcribing…" placeholder
- macOS install steps in the README; signed macOS release builds via a stable self-signed certificate

### Changed
- BYOK-only build: cloud account, subscription, and telemetry surfaces removed; no auto-update
- Tightened LLM polish prompt; typed output ends with a trailing space
- Always start in the tray; the `start_minimized` setting was dropped
- Capsule trims post-recording stage chrome and stays at polishing width to avoid a mid-exit clip
- Capsule respects `capsule_auto_hide` on fresh launch and asserts hidden on first mount
- README translations removed; remaining Chinese test comments translated
- Discord references removed from documentation

### Fixed
- macOS capsule overlay now behaves correctly across hide, multi-monitor, and fullscreen Spaces
- macOS accessibility permission prompt no longer crashes (uses the real `kAXTrustedCheckOptionPrompt` constant)

## [0.1.0] - 2026-02-26

### Added
- Initial open-source release under MIT license
- Global hotkey voice recording with hold-to-record and toggle modes
- Floating capsule widget — always-on-top, draggable, with recording/transcribing/polishing states
- 6 STT providers: Deepgram Nova-3, AssemblyAI, OpenAI Whisper, Groq Whisper, GLM-ASR, SiliconFlow
- 11 LLM providers: OpenAI, DeepSeek, Zhipu, Claude, Gemini, Moonshot, Qwen, Groq, Ollama, OpenRouter, SiliconFlow
- Real-time streaming keyboard output — text appears character-by-character as the LLM generates it
- Clipboard output mode as alternative to keyboard simulation
- Selected text context — highlight text before recording to give the LLM additional context
- Translation mode — speak in one language, output in another (20+ target languages)
- Custom dictionary for domain-specific terms and proper nouns
- Per-app detection — adapts formatting based on the active application
- Local history with full-text search and date grouping
- Dark / light / system theme with smooth transitions
- Onboarding wizard for first-time setup
- System tray with quick actions (show/hide, start recording, quit)
- Auto-start on login
- BYOK (Bring Your Own Key) only — no cloud account, subscription, telemetry, or auto-update
- Cross-platform support: Windows, macOS, Linux
- CI/CD with automated builds for all three platforms
