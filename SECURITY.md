# Security

Vellora is an early Windows beta. Security fixes are developed against the latest repository version; older binaries are not independently maintained.

## Report a vulnerability

Use [GitHub's private vulnerability reporting](https://github.com/Felix561/vellora-windows/security/advisories/new) on this repository. This confidential reporting route is enabled. Please include the affected version, reproduction steps, expected impact, and a minimal example using dummy data. Do not include real API keys, personal transcripts, recordings, clipboard contents, or unredacted diagnostics.

If private reporting is unavailable, open an issue requesting a private contact without publishing the vulnerability or sensitive data. The project does not currently offer a response-time guarantee.

The source is public as an early Windows beta; the original development/audit repository remains private. Public installers are deferred. See [publication status](docs/PUBLICATION_STATUS.md).

## Data and trust boundaries

- OpenAI API keys are stored in Windows Credential Manager and should never be committed or added to frontend environment variables.
- Dictation audio is sent to OpenAI; optional cleanup sends transcript text to OpenAI. These requests originate from the Rust backend.
- Transcript history and settings are stored locally. Local history is not encrypted by Vellora, so use Windows account protection and disk encryption where appropriate.
- The frontend loads bundled assets. Production CSP blocks external scripts, frames, and direct remote network requests. Inline styles remain enabled for chart and audio-level rendering; scripts do not permit inline execution or evaluation.
- Frontend Tauri permissions grant only event listening and listener removal. Sensitive operations are implemented by application Rust commands.
- Dictation uses a global keyboard hook and simulates paste input into the focused Windows application. Review the target window before recording sensitive text.
- Windows startup is configurable in the GUI. Vellora does not need administrator privileges for normal use.

See [privacy documentation](docs/PRIVACY.md) for the current storage, retention, and deletion behavior.

## Dependency and release checks

Public CI runs the production frontend build, rendered GUI checks, locked Rust tests and lints, npm and RustSec vulnerability checks, and a redacted scan of the complete Git history. Release executable/installer compilation and artifact uploads are disabled for this public repository. Actions and secret-scanner downloads are pinned to commits or verified checksums. CI does not require an OpenAI API key or send dictation requests.

Release-candidate checks remap compiler build paths and inspect the unpacked EXE for profile/source roots. Archive inspection checks the exact reviewed Tauri NSIS marker patch, directly scans the extracted payload and verifies all three committed license/notice resources, without running the installer. Private review artifacts include checksums and covered-source archives. Installer artifact uploads are disabled when the repository is public; a public binary release needs a separate approval. Do not reuse the flagged local 0.2.19 installer.

Never disable TLS certificate verification to make a build pass. If a trusted corporate proxy is required, configure its CA certificate in the local operating system or the local npm/Cargo trust settings. Machine-specific certificates and proxy settings must stay outside the repository.

Automated scans reduce risk but are not a penetration test. Before distributing a public installer, validate it on a clean Windows account and follow the release checklist in [CONTRIBUTING.md](CONTRIBUTING.md).
