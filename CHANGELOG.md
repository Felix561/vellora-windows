# Changelog

Vellora is a Windows-only early beta. Entries describe private review versions; they are not announcements of a public installer release. For validation and remaining release gates, see the [audit](docs/AUDIT.md) and [roadmap](docs/ROADMAP.md).

## Public source opening - October 6, 2026

- Published a clean-history Windows source beta under MIT as `Felix561/vellora-windows`; the development archive stays private.
- Updated README, issue forms and in-app support links for the public source repository.
- Public CI validates source and frontend assets without building or uploading a release executable/installer. Public EXE releases remain deferred.
- Enabled confidential vulnerability reporting.

## 0.2.21 — October 6, 2026

### Fixed

- Releasing and re-pressing only Ctrl or only Win during a held recording no longer enters hands-free mode. Locking requires both modifier groups to be fully released between taps.
- Resuming a hold cancels its pending stop; an earlier release timer cannot shorten a later release or claim another recording.
- Shortcut events queued before recording resets cannot start an unresponsive recording after processing, failed microphone startup or an automatic stop.
- Help and guided setup explain releasing both keys between hands-free taps.
- Updated the transitive development dependency `source-map-js` to 1.2.2 for GHSA-68fv-2mgg-jv7q; application runtime dependencies are unchanged.

### Verification

- Added targeted shortcut/session regressions and a synthetic microphone worker that becomes ready and then fails.
- Expanded isolated Chromium GUI checks for keyboard focus, long text, forced colors, reduced motion and viewport/DPR reflow in both appearances. These use synthetic IPC and clipboard data, with provider/network requests blocked.
- An installed update is separate from automated validation; native microphone, clipboard, screen-reader and clean-account checks remain release gates.

## 0.2.20 — October 4, 2026

### Fixed

- Bounded retries while a microphone driver worker is blocked, preserved originating-session ownership for late recording events, and checked cancellation before continuing device startup.
- Shortcut recovery after keyboard inactivity preserves locked and physically held recordings; failed accepted recording starts clear phantom shortcut state.
- Serialized partial settings changes prevent navigation/model/theme saves from undoing newer preferences such as history-off.
- Stale asynchronous reads cannot replace newer state or reintroduce deleted text; changing the displayed transcript cancels its pending paste countdown.
- Recording and unavailable-key indicators now reflect their actual state.

### Added

- Publication privacy, code, licensing, model-lifecycle and peer-project reviews, with explicit approval and native testing decisions.
- External NSIS/plugin/WebView2 loader notices bundled with the existing license resources.
- Hosted-build compiler path remapping, redacted binary checks, installer payload/notice verification, checksums and exact covered-source review archives.
- Eleven additional frontend and nine additional Rust regression cases; review artifact uploads remain private-only.
- Newly generated logo, Windows icons and refreshed synthetic screenshots, with exact generation prompts, inputs and asset hashes recorded.

### Review status

- Existing local 0.2.19 installers are not public-release candidates because their compiled executable contains developer build paths and predates these fixes/notices.
- Current replacement-artwork provenance is documented. Earlier unverified artwork remains in private history; the proposed public source uses a separate clean-history snapshot.
- No installed application data or selected model was changed. No public repository or installer release is announced.

## 0.2.19 — October 2, 2026

### Added

- In-app Help with version/license information, privacy and troubleshooting guidance, and support/documentation links.
- A simple first-use setup guide that can be reopened from Help & About. It explains key storage, microphone setup, the recording shortcut, and destination focus.
- A local check of default-microphone availability/configuration; no sound recording or provider request is made by the check.
- Copyable support diagnostics limited to non-sensitive version, platform, and state fields.
- Automated frontend regression checks using synthetic data, without paid API calls or access to personal credentials/history.
- Windows troubleshooting, bug/feature issue forms, and a user-first README.

### Unchanged limitations

- Local microphone checks do not prove sound capture, permissions, or transcription quality.
- Windows is the only supported operating system. No offline provider, microphone picker, history search/export, or public release is introduced.
- Installers remain private review builds; signing, clean-account installation, real-device testing, and other public-beta gates remain open.

## 0.2.18 — October 1, 2026

### Improved

- Click-through status indicator, persisted Notebook/Windows Classic appearances, and GUI startup control.
- Recording limits, microphone error feedback, temporary-audio cleanup, session stop handling, and request error handling.
- Transcript recovery when history is off, cancellable delayed re-paste, deletion confirmations, and honest save/error feedback.
- API-key protection, production CSP and frontend permissions, atomic settings saves, dependency updates, and exact-version third-party notices.
- Private repository preparation, Windows CI validation, dependency audits, and secret scanning.
- Dependency update grouping and CI triggers to reduce duplicate routine update runs.

This summarizes the private preparation work; older OpenFlow/Vellora development versions do not have a complete release-by-release changelog.
