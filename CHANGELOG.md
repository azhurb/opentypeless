# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/), and this project adheres to [Semantic Versioning](https://semver.org/).

## Fork

This repository is a fork of [Tover0314/opentypeless](https://github.com/tover0314-w/opentypeless). The entry for `0.1.0` describes the upstream baseline; `0.2.0` is the fork's first release, marking the BYOK-only direction and the changes listed below.

## [Unreleased]

### Changed
- Output is now exclusively clipboard-paste with the user's prior clipboard snapshotted and restored. The "keyboard simulation" output mode and the macOS Accessibility prompt that gated it are removed; users no longer need to grant Accessibility for normal dictation.
- Foreground app detection on macOS now also captures the bundle identifier, used to drive per-target paste behavior.

### Added
- Per-target paste chunking for terminal-hosted CLIs (Claude CLI, Codex CLI, Gemini CLI). When the foreground app is a recognised terminal emulator or IDE terminal panel (Terminal.app, iTerm2, Warp, Ghostty, Kitty, Alacritty, Hyper, WezTerm, VS Code, Cursor, Windsurf, JetBrains family) and the window title matches a known CLI name, the paste is split into chunks with brief delays so the CLI's input buffer doesn't drop characters.

### Removed
- "Output Mode" setting in Settings → General and the associated macOS Accessibility permission card.
- Streaming-as-you-type output: LLM polish output now lands as a single paste once polish completes. The capsule still renders the live polish indicator from `llm:chunk` events.

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
