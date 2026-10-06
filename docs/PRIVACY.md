# Privacy and local data

This describes the Vellora 0.2.20 private review candidate, reviewed October 4, 2026. Vellora uses your OpenAI API account. The reviewed source has no application analytics service, hosted transcript account, or background transcript synchronization. Publication-file and Git-history checks are recorded separately in [PUBLICATION_PRIVACY.md](PUBLICATION_PRIVACY.md).

## What leaves the PC

After a usable recording, the Rust backend sends WAV audio, the selected model and optional language code over HTTPS to OpenAI's audio transcription endpoint. Optional cleanup sends the resulting text and cleanup instructions to OpenAI's Responses endpoint. Cleanup is disabled by default and requests use `store: false`.

The backend attaches the API key to requests. The frontend can save, remove, and check the presence of a key but cannot retrieve its value. Requests reject redirects and use explicit timeouts. Diagnostics omit remote response bodies; keys, recordings and transcripts are not deliberately logged.

`store: false` controls response application storage; it does not grant organization Zero Data Retention. OpenAI documents endpoint-specific retention, including no application state or abuse-monitoring retention for audio transcription and separate Responses rules. Account settings and applicable provider policies still matter. Consult [OpenAI's data controls](https://developers.openai.com/api/docs/guides/your-data) for current details. Vellora uses `api.openai.com` and does not configure regional endpoints.

## What stays on the PC

| Data | Location | Retention |
| --- | --- | --- |
| API key | Windows Credential Manager, service `Vellora`, username `openai_api_key` | Until removed through Settings or Credential Manager |
| Settings | `%APPDATA%/Vellora/settings.json` | Until changed or removed |
| Raw/final text, models, timestamps, durations, paste outcome and sanitized errors | `%APPDATA%/Vellora/vellora.db` | Until deleted from History or removed manually |
| Latest completed transcript | Process memory | Until replaced, deleted/cleared, or exit; retained even with history saving disabled |
| Temporary WAV | `%TEMP%/Vellora/audio/vellora-<random>.wav` | Removed after normal processing/error/cancellation; startup removes owned stale files older than 24 hours |
| Recovered invalid settings | `%APPDATA%/Vellora/settings-invalid-*.json` | Preserved until manually removed |
| Credential import marker | `%APPDATA%/Vellora/credential-migration-complete` | Prevents legacy credentials being re-imported after removal |

Vellora does not encrypt SQLite history or settings. Other software with access to your Windows account may read this data. Windows account protection and disk encryption are separate protections. A forced shutdown can leave temporary recordings until a later cleanup. Cleanup skips junctions/symlinks and unrelated filenames.

Disabling **Save local history** affects later dictations; it does not delete earlier entries. A dictation captures its processing, delivery and history options when recording stops. Changing those options while it is transcribing does not cancel that operation or change its captured options. Clearing history during processing can still be followed by that operation adding its result. Wait until processing finishes, or quit Vellora, before changing retention/delivery options and deleting data when you need a definite boundary. Latest text stays in memory so clipboard failures do not lose your work. Diagnostics and GUI state remain in memory until replaced or exit.

## Setup, help and support information

The guided setup stores only a completion/skip marker (`vellora.setup.v1`) in the main window's local WebView storage. Its microphone availability check reads the default device/configuration locally, without recording sound or making an OpenAI request. The optional practice dictation is a real, manually initiated request with normal API billing.

**Copy support information** copies an allowlisted summary after an explicit click. It includes the application version, Windows/architecture and enum/boolean readiness states. It excludes keys, audio, transcript text, device names, account names, personal paths and free-form errors. A preview is shown; copying replaces the clipboard. Vellora does not upload the summary. Fixed help links open in Windows or the browser only when clicked.

## Clipboard and destination apps

If copying is enabled, transcript text enters Windows clipboard. Clipboard history, synchronization, other apps and the destination can retain it independently.

With copying disabled and automatic paste enabled, Vellora temporarily uses the clipboard and attempts to restore previous plain text or empty state after a short delay. A sequence-number check avoids overwriting a newer update. This is best effort, and it does not preserve rich text/other formats when plain text is also offered. Pure non-text content causes auto-paste to be skipped. Use manual copying when exact clipboard preservation matters.

Vellora checks the top-level destination window before simulating Ctrl+V. It does not track caret movement, browser-tab changes, or focus changes within that same window. Successful input injection does not prove the app accepted or stored text. Content pasted into another app is outside Vellora's deletion controls.

## Deletion and legacy data

History offers individual deletion and clear-all confirmation. These remove rows from the active database and clear matching/latest in-memory text. SQLite secure deletion is enabled. This does not guarantee forensic erasure from SSDs, snapshots, backups or other copies.

Removing the key deletes the active Vellora credential and prevents legacy re-import. It does not revoke the key at OpenAI or remove the legacy OpenFlow credential. Revoke compromised keys in your OpenAI account.

Migration copies settings/history and may import an OpenFlow credential once. Old `%APPDATA%/OpenFlow` data and its Credential Manager entry remain deliberately. History deletion does not remove these backups, old-version temporary audio, clipboard history, or provider records. To remove all local copies, exit both apps and review those locations separately. Removing the active Vellora database while keeping legacy history can allow that history to be copied again at startup.

Uninstall retention has not yet been tested on a clean account. Do not assume uninstall removes personal data. The [roadmap](ROADMAP.md) includes a complete data-removal flow and installer validation.
