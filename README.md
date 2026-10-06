# Vellora

<img src="src/assets/brand/vellora-symbol.png" alt="Vellora logo" width="80" height="80" />

**Windows dictation with your own OpenAI API key.**

Hold **Ctrl + Win**, speak, and release. Vellora transcribes the recording through OpenAI and can paste the text into your focused app. It runs in the system tray, keeps text recoverable when paste fails, and offers Notebook and Windows Classic appearances.

[![Windows](https://img.shields.io/badge/platform-Windows-blue)](#requirements)
[![MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)
[![Beta](https://img.shields.io/badge/status-early%20source%20beta-orange)](#install)
[![Windows checks](https://img.shields.io/badge/CI-Windows%20source%20checks-blue)](https://github.com/Felix561/vellora-windows/actions/workflows/ci.yml)

[Run from source](#development) · [First dictation](#your-first-dictation) · [Screenshots](#screenshots) · [Troubleshooting](docs/TROUBLESHOOTING.md) · [Privacy](docs/PRIVACY.md) · [Contributing](CONTRIBUTING.md)

**Early Windows source beta.** Vellora is MIT licensed and open for inspection and contribution. Windows is the only supported platform. This repository publishes source code; there are no public installers or executable downloads yet. Clean-account installation and real-device release checks remain open. See the [publication review](docs/PUBLICATION_REVIEW.md) for evidence and remaining work. The CI badge links to source checks rather than claiming a particular run has passed.

## Why Vellora?

- **A focused Windows workflow:** hold-to-record or hands-free dictation, a tray app, a click-through status indicator, and startup control in Settings.
- **Your model choice:** Whisper, GPT-4o mini Transcribe, GPT-4o Transcribe, or GPT-4o Transcribe Diarize. Optional text cleanup is off by default.
- **Recoverable text:** automatic paste, copying, delayed re-paste, and optional local history with deletion controls.
- **Two appearances:** a Notebook interface or a Windows Classic interface inspired by Windows 95/98.
- **A short setup path:** reopenable guided setup, a local microphone availability check, and built-in help.
- **An inspectable client:** React and Rust source, automated checks, and documented data flows and limitations.

**API usage is billed to your OpenAI account.** Audio is sent to OpenAI; optional cleanup sends transcript text too. Vellora currently has no offline transcription provider. Read the [privacy details](docs/PRIVACY.md) before using sensitive material.

## Screenshots

| Notebook — Dashboard | Windows Classic — Dashboard |
| --- | --- |
| [![Notebook Dashboard with synthetic demo data](docs/images/dashboard-notebook.png)](docs/images/dashboard-notebook.png) | [![Windows Classic Dashboard with synthetic demo data](docs/images/dashboard-classic.png)](docs/images/dashboard-classic.png) |

Both appearances have the same functionality. These Vellora 0.2.20 screenshots use **synthetic demo data**, including transcripts, statistics, costs, and simulated saved-key status. No real API key or personal history was used. Click an image to view it at full size; see [image provenance](docs/images/README.md).

<details>
<summary>More screenshots: settings, appearances, history, models, help, and guided setup</summary>

| Page | Notebook | Windows Classic |
| --- | --- | --- |
| General settings | [![Notebook General settings with synthetic demo data](docs/images/settings-notebook.png)](docs/images/settings-notebook.png) | [![Windows Classic General settings with synthetic demo data](docs/images/settings-classic.png)](docs/images/settings-classic.png) |
| Appearance | [![Notebook Appearance settings with synthetic demo data](docs/images/appearance-notebook.png)](docs/images/appearance-notebook.png) | [![Windows Classic Appearance settings with synthetic demo data](docs/images/appearance-classic.png)](docs/images/appearance-classic.png) |
| History | [![Notebook History with synthetic demo transcripts](docs/images/history-notebook.png)](docs/images/history-notebook.png) | [![Windows Classic History with synthetic demo transcripts](docs/images/history-classic.png)](docs/images/history-classic.png) |
| AI Models | [![Notebook AI Models with synthetic demo data](docs/images/models-notebook.png)](docs/images/models-notebook.png) | [![Windows Classic AI Models with synthetic demo data](docs/images/models-classic.png)](docs/images/models-classic.png) |
| Help & About | [![Notebook Help and About with synthetic demo data](docs/images/help-notebook.png)](docs/images/help-notebook.png) | [![Windows Classic Help and About with synthetic demo data](docs/images/help-classic.png)](docs/images/help-classic.png) |
| Guided setup | [![Notebook guided setup with synthetic demo data](docs/images/setup-notebook.png)](docs/images/setup-notebook.png) | [![Windows Classic guided setup with synthetic demo data](docs/images/setup-classic.png)](docs/images/setup-classic.png) |

</details>

## Requirements

- Windows 10/11 with Microsoft Edge WebView2.
- A working microphone and permission for desktop apps to use it.
- Internet access and an OpenAI API key with API billing enabled.

Vellora does not need administrator privileges for normal use.

## Install

There is no public installer or executable download yet. To try Vellora, follow the [development instructions](#development) to run it from source on Windows.

Executable releases are deferred until a separate maintainer decision and the remaining [release checks](docs/ROADMAP.md), including clean-account installation, device testing, signing/checksums, and redistribution requirements. Private review artifacts are not a public download channel.

## Your first dictation

1. Follow the setup guide and save your OpenAI API key, or use **Settings > General**. The saved-key indicator confirms local storage; it does not test billing or provider access.
2. Check your Windows microphone input. The guide's local check confirms default-device availability/configuration. It does not record sound, validate permissions, or make a paid API request.
3. Choose a transcription model. Leave optional cleanup off for your first try.
4. Focus an editable field in a normal Windows app such as Notepad. Hold **Ctrl + Win**, speak a short non-sensitive sentence, then release both keys.
5. Wait for transcription. With automatic paste enabled, Vellora attempts to paste into the original window. Otherwise, use **Copy** on the Dashboard or in History.

Keep the same destination field focused while dictating. Automatic paste checks the top-level window, not a browser tab or caret location within it.

| Action | Control |
| --- | --- |
| Record while holding the shortcut | Hold **Ctrl + Win**; release to transcribe |
| Record hands-free | Double-tap **Ctrl + Win**, releasing both keys between taps; press again to stop |
| Re-paste into another field | Select **Paste in 3s**, focus the destination, or cancel |
| Change appearance or launch at sign-in | **Settings > Appearance** or **Settings > General > Windows startup** |
| Exit completely | Tray menu > **Quit**; closing the window keeps the tray app running |

New users without a saved key see the guide automatically. You can skip it and reopen it through **Help & About > Open guided setup**. Help also offers version information and **Copy support information**: a limited summary without keys, audio, transcripts, device names, personal paths, or free-form errors. See [troubleshooting](docs/TROUBLESHOOTING.md) if recording or paste does not work.

## Privacy and current limitations

Settings and text history are stored under `%APPDATA%/Vellora`; the API key is stored in Windows Credential Manager. Vellora does not encrypt local history. Disabling history affects future entries and keeps the latest transcript recoverable in memory.

Temporary audio is removed after normal processing or failure. A forced shutdown may leave a file; the next launch removes owned recordings older than 24 hours. Earlier OpenFlow data can remain separately, and uninstall is not a verified data-removal method. Read [privacy and deletion details](docs/PRIVACY.md).

- Recordings stop automatically after five minutes or before reaching the upload size limit.
- Diarize currently returns plain text; speaker labels are not displayed.
- Clipboard restoration is best effort and preserves plain text only. Rich formatting can be lost; pure non-text clipboard content causes automatic paste to be skipped when preservation is requested. Slow paste consumers can race restoration.
- Windows can reject paste into elevated applications. Input injection succeeding does not prove a destination accepted the text.
- Cost charts estimate saved successful transcripts; they are not a billing ledger.
- Current provider model IDs have scheduled retirement dates. See the [model lifecycle review](docs/MODEL_LIFECYCLE.md) for deadlines and the migration work still required; replacement models have not been integrated or validated.
- A running dictation uses options captured when recording stops. Wait until idle or quit before changing retention/delivery and deleting data for a definite boundary.
- Microphone selection, history search/export/pagination, complete local-data removal, and public installer signing remain on the [roadmap](docs/ROADMAP.md).

## Development

Install Node.js 22.22.2+, Rust 1.95.0, and Visual Studio C++ Build Tools with the Windows SDK. Follow [Tauri's Windows prerequisites](https://v2.tauri.app/start/prerequisites/#windows), then run these commands from the repository root:

```powershell
git clone https://github.com/Felix561/vellora-windows.git
cd vellora-windows
npm ci
npm run tauri:dev
```

No API key is required to build or run automated tests. Configure your key through Settings only when testing real dictation. Never put a key in source code or a frontend environment variable.

### Checks

```powershell
npm run check:frontend
npm run build
npm run check:rust
npm run test:rust
```

`check:frontend` runs linting, app/test type checks, and frontend regression tests. To run just the tests, use `npm test` (or `npm run test:watch` during development).

The [public GitHub Actions workflow](https://github.com/Felix561/vellora-windows/actions/workflows/ci.yml) validates the Windows source with frontend and Rust checks, synthetic rendered-GUI tests, dependency audits, and secret scanning. It does not build or upload release installers or publish executable downloads. See [CONTRIBUTING.md](CONTRIBUTING.md) for dependency audits, optional local native build instructions, and the release checklist; see [GUI_TESTING.md](docs/GUI_TESTING.md) for synthetic interface tests. Build an executable locally only when needed for native or installation testing.

Keep TLS verification enabled. On a managed network, configure the trusted proxy CA locally rather than changing repository security settings.

## Project information

- [Changelog](CHANGELOG.md) and [prioritized roadmap](docs/ROADMAP.md)
- [Troubleshooting](docs/TROUBLESHOOTING.md) and [privacy](docs/PRIVACY.md)
- [Audit and validation](docs/AUDIT.md) and [contributing](CONTRIBUTING.md)
- [Publication review](docs/PUBLICATION_REVIEW.md) and [source-publication copy](docs/PUBLICATION_COPY.md)
- [Comparison with other dictation projects](docs/PROJECT_COMPARISON.md) and [model lifecycle](docs/MODEL_LIFECYCLE.md)
- [Security reporting](SECURITY.md)
- [MIT license](LICENSE) and [third-party notices](THIRD_PARTY_NOTICES.md)
- [Native component notices](THIRD_PARTY_NOTICES_NATIVE.md) and [licensing review](docs/LICENSING_REVIEW.md)
- [Current logo generation and asset provenance](docs/ARTWORK_PROVENANCE.md)

Built with Tauri 2, Rust, React, TypeScript, and Vite. Originally named OpenFlow; existing local data can migrate to Vellora without deleting the old copy. Vellora currently focuses on Windows dictation rather than meetings, account synchronization, or support for other operating systems.
