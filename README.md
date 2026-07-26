<p align="center">
  <img src="src-tauri/icons/128x128@2x.png" width="128" height="128" alt="OpenTypeless Logo" />
</p>

<h1 align="center">OpenTypeless</h1>

<p align="center">
  Open-source AI voice input for desktop. Speak naturally, get polished text in any app.
</p>

<p align="center">
  Whether you're writing emails, coding, chatting, or taking notes — just press a hotkey,<br/>
  speak your mind, and OpenTypeless transcribes and polishes your words with AI,<br/>
  then types them directly into whatever app you're using.
</p>

<p align="center">
  <a href="https://github.com/azhurb/opentypeless/actions/workflows/ci.yml"><img src="https://github.com/azhurb/opentypeless/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="https://github.com/azhurb/opentypeless/releases"><img src="https://img.shields.io/github/v/release/azhurb/opentypeless?color=2ABBA7" alt="Release" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/azhurb/opentypeless" alt="License" /></a>
  <a href="https://github.com/azhurb/opentypeless/stargazers"><img src="https://img.shields.io/github/stars/azhurb/opentypeless?style=social" alt="Stars" /></a>
</p>

<p align="center">
  <img src="docs/images/demo.gif" width="720" alt="OpenTypeless Demo" />
</p>

<details>
<summary>More screenshots</summary>

<p align="center">
  <img src="docs/images/app-main-light.png" width="720" alt="OpenTypeless Main Window" />
</p>

| Settings | History |
|---|---|
| <img src="docs/images/app-settings.png" width="360" /> | <img src="docs/images/app-history.png" width="360" /> |

</details>

---

## Why OpenTypeless?

| | OpenTypeless | macOS Dictation | Windows Voice Typing | Whisper Desktop |
|---|---|---|---|---|
| AI text polishing | ✅ Multiple LLMs | ❌ | ❌ | ❌ |
| STT provider choice | ✅ 6+ providers | ❌ Apple only | ❌ Microsoft only | ❌ Whisper only |
| Works in any app | ✅ | ✅ | ✅ | ❌ Copy-paste |
| Translation mode | ✅ | ❌ | ❌ | ❌ |
| Open source | ✅ MIT | ❌ | ❌ | ✅ |
| Cross-platform | ✅ Win/Mac/Linux | ❌ Mac only | ❌ Windows only | ✅ |
| Custom dictionary | ✅ | ❌ | ❌ | ❌ |
| Self-hostable | ✅ BYOK | ❌ | ❌ | ✅ |

## Features

- 🎙️ Global hotkey recording — hold-to-record or toggle mode
- 💊 Floating capsule widget that stays on top
- 🗣️ 6+ STT providers: Deepgram, AssemblyAI, Whisper, Groq, GLM-ASR, SiliconFlow
- 🤖 Text polishing via multiple LLMs: OpenAI, DeepSeek, Claude, Gemini, Ollama, and more
- ⚡ Streaming output — text appears as the LLM generates it
- ⌨️ Keyboard simulation or clipboard output
- 📝 Highlight text before recording to give the LLM context
- 🌐 Translation mode: speak in one language, output in another (20+ languages)
- 📖 Custom dictionary for domain-specific terms
- 🔍 Per-app detection to adapt formatting
- 📜 Local history with full-text search — optional, with automatic cleanup after 7/30/90 days
- 🌗 Dark / light / system theme
- 🚀 Auto-start on login

> [!TIP]
> **Recommended Configuration for Best Experience**
>
> | | Provider | Model |
> |---|---|---|
> | 🗣️ STT | Groq | `whisper-large-v3-turbo` |
> | 🤖 AI Polish | Google | `gemini-2.5-flash` |
>
> This combo delivers fast, accurate transcription with high-quality text polishing — and both offer generous free tiers.

## Download

Download the latest version for your platform:

**[Download from Releases](https://github.com/azhurb/opentypeless/releases)**

| Platform | File |
|----------|------|
| Windows | `.msi` installer |
| macOS (Apple Silicon) | `.dmg` |
| macOS (Intel) | `.dmg` |
| Linux | `.AppImage` / `.deb` |

## Installation

### macOS

Builds are signed with a self-signed certificate (not a paid Apple Developer ID), so macOS quarantines them on download. On first install, strip the quarantine attribute:

1. Open the `.dmg` and drag **OpenTypeless** into `/Applications`.
2. In Terminal, run:
   ```bash
   xattr -cr /Applications/OpenTypeless.app
   ```
3. Launch the app. Grant **Microphone** and **Accessibility** permissions when prompted.

When upgrading to a new release, repeat step 2 (each download gets a fresh quarantine flag). Accessibility and Microphone grants persist across upgrades — no need to re-grant.

### Windows and Linux

Open the installer / AppImage / `.deb` and follow the standard install prompts.

## Prerequisites

- [Node.js](https://nodejs.org/) 20+
- [Rust](https://rustup.rs/) (stable toolchain)
- Platform-specific dependencies for Tauri: see [Tauri Prerequisites](https://v2.tauri.app/start/prerequisites/)

## Getting Started

```bash
# Install dependencies
npm install

# Run in development mode
npm run tauri dev

# Build for production
npm run tauri build
```

The built application will be in `src-tauri/target/release/bundle/`.

## Configuration

All settings are accessible from the in-app Settings panel:

- **Speech Recognition** — choose STT provider and enter your API key
- **AI Polish** — choose LLM provider, model, and API key
- **General** — hotkey, theme, auto-start
- **Dictionary** — add custom terms for better transcription accuracy

API keys are never kept in a plaintext settings file. On Windows they go to Credential Manager and on Linux to the Secret Service; on macOS they go to a file only your account can read, because the Keychain would ask for your password after every app update unless the project pays for an Apple Developer ID. If you are upgrading from a version that kept them in `settings.json`, they move across on first launch. All STT/LLM requests go directly from your machine to the provider you configure. There is no cloud account, subscription, telemetry, or auto-update — this fork is BYOK-only.

## Architecture

For deeper repository-local architecture docs, start at [docs/index.md](docs/index.md).

**Data Flow Pipeline:**

```
Microphone → Audio Capture → STT Provider → Raw Transcript → LLM Polish → Clipboard Paste
```

```
src/                  # React frontend (TypeScript)
├── components/       # UI components (Settings, History, Capsule, etc.)
├── hooks/            # React hooks (recording, theme, Tauri events)
├── lib/              # Utilities (API client, router, constants)
└── stores/           # Zustand state management

src-tauri/src/        # Rust backend
├── audio/            # Audio capture via cpal
├── stt/              # STT providers (Deepgram, AssemblyAI, Whisper-compat)
├── llm/              # LLM providers (OpenAI-compatible)
├── output/           # Clipboard-paste output with per-target chunking
├── storage/          # Config (tauri-plugin-store) + history/dictionary (SQLite)
├── app_detector/     # Detect active application for context
├── pipeline.rs       # Recording → STT → LLM → Output orchestration
└── lib.rs            # Tauri app setup, commands, hotkey handling
```

## Roadmap

- [ ] Plugin system for custom STT/LLM integrations
- [ ] Improved multi-language STT accuracy and dialect support
- [ ] Voice commands (e.g. "delete last sentence")
- [ ] Customizable hotkey combinations
- [ ] Improved onboarding experience
- [ ] Mobile companion app

## FAQ

**Is my audio sent to the cloud?**
Audio goes directly to whichever STT provider you configure (e.g., Groq, Deepgram). No data is routed through OpenTypeless servers — there is no telemetry or background reporting in this fork.

**Can I use it offline?**
With a local STT provider (Whisper via Ollama) and a local LLM (Ollama), the app works entirely offline. No internet connection needed.

**Which languages are supported?**
STT supports 99+ languages depending on the provider. AI polish and translation support 20+ target languages.

**Is the app free?**
Yes. Bring your own provider API keys.

## Community

- 🐛 [Issue Tracker](https://github.com/azhurb/opentypeless/issues) — Bug reports and feature requests
- 📖 [Contributing Guide](CONTRIBUTING.md) — Development setup and guidelines
- 🔒 [Security Policy](SECURITY.md) — Report vulnerabilities responsibly
- 🧭 [Vision](VISION.md) — Project principles and roadmap direction

## Contributing

Contributions are welcome! See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup and guidelines.

Looking for a place to start? Check out issues labeled [`good first issue`](https://github.com/azhurb/opentypeless/labels/good%20first%20issue).

## Star History

<a href="https://star-history.com/#azhurb/opentypeless&Date">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/svg?repos=azhurb/opentypeless&type=Date&theme=dark" />
    <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/svg?repos=azhurb/opentypeless&type=Date" />
    <img alt="Star History Chart" src="https://api.star-history.com/svg?repos=azhurb/opentypeless&type=Date" />
  </picture>
</a>

## Built with Claude Code

This entire project was built in a single day using [Claude Code](https://claude.com/claude-code) — from architecture design to full implementation, including the Tauri backend, React frontend, CI/CD pipeline, and this README.

## Credits

This is a personal fork of the original [OpenTypeless](https://github.com/tover0314-w/opentypeless) by [Tover0314](https://github.com/tover0314-w), who built the initial app. This fork strips the cloud / subscription / account features and keeps only the local BYOK pipeline. All credit for the original architecture and implementation goes to the upstream author; subsequent changes here are mine.

## License

[MIT](LICENSE) — see the upstream copyright notice in [LICENSE](LICENSE).
