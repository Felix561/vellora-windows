# Publication roadmap

Updated October 6, 2026 for the 0.2.21 public source beta. Windows remains the only supported operating system. P0 records the reviewed clean source handoff; source publication is approved and the existing audit repository remains private. See [publication status](PUBLICATION_STATUS.md). P1 blocks a public beta installer or requires an explicit documented release decision; P2 improves the next beta. Each task has acceptance criteria. The concrete approval package is [PUBLICATION_REVIEW.md](PUBLICATION_REVIEW.md).

## Completed preparation

- [x] MIT license, synchronized versions, README, contribution/security/privacy documentation.
- [x] Private repository preparation, preserved history, secret scans and personal-data exclusions.
- [x] Locked builds, Rust formatting/lints/tests, dependency audits, pinned CI and Dependabot.
- [x] Production CSP, minimal capabilities, main-window guards for sensitive commands/events.
- [x] Click-through indicator, GUI startup control, persisted Notebook/Windows Classic appearances.
- [x] Settings validation/atomic saves/invalid-file backup and recovery warning.
- [x] Microphone startup/error reporting, bounded recordings, scoped temporary files/stale cleanup.
- [x] Session-specific stop handling, hotkey startup feedback and stale-timer protection.
- [x] Robust cleanup parsing/raw-text fallback, store:false, sanitized errors and bounded retries.
- [x] Honest error feedback, delete/clear history, history-off recovery and cancellable delayed re-paste.
- [x] Synthetic GUI checks, keyboard confirmations, contrast and reduced-motion improvements.
- [x] Exact-version third-party notice inventory and installer license/notice resources.
- [x] User-first README, Windows troubleshooting, changelog, and privacy-conscious bug/feature forms.
- [x] In-app Help/About, reopenable first-use setup, and allowlisted support diagnostics.
- [x] Local default-microphone availability/configuration check; no sound capture or API request during the check.
- [x] Automated frontend regression checks with synthetic data; native hardware and installer checks remain separate.
- [x] Current source/history/images and repository-scoped metadata/log privacy review; intentional credits and scan limits recorded.
- [x] Microphone worker/session, stale shortcut, asynchronous UI/settings and transcript countdown fixes with targeted regressions.
- [x] Native component notices, compiler path remapping, binary/payload verification and exact covered-source companion automation.
- [x] Peer-project comparison, revised presentation, AI/Windows licensing review and proposed post-approval publication copy.
- [x] Newly generated logo with recorded prompts/inputs/hashes, replacement Windows icons and refreshed synthetic screenshots.
- [x] Partial-modifier shortcut, resumed-hold timers and stale queued-start corrections with synthetic regressions.
- [x] Hosted rendered checks of both appearances, selected keyboard/focus transitions, long text, forced colors, reduced motion and viewport/DPR reflow; native checks remain separate.
- [x] One owner-approved existing-account update to 0.2.21, with installed payload/notices/version and startup preservation verified.
- [x] Owner confirmation after that update that the reported partial-modifier shortcut sequence now works.

## P0: completed source publication preparation

| ID | Task | Acceptance criteria | Private review result |
| --- | --- | --- | --- |
| PUB-01 | Keep source/history free of secrets and personal data | Redacted history/staged scans pass; no personal databases, recordings, screenshots, certificates or machine-specific paths in publication files. | Passed in the reviewed scope; intentional credits and exclusions documented. |
| PUB-02 | Validate pushed application candidate and final handoff | Windows CI build/lints/tests, dependency audits and history scan pass on the identified application commit; documentation/formatting follow-ups receive relevant checks. | 0.2.21 candidate `62b9297` passed all three jobs in run 37471723603, including rendered GUI and actual installer checks. Final documentation/tree/privacy checks recorded separately. |
| PUB-03 | Prepare independent clean publication history | Reviewed source snapshot has no earlier unverified logo/icon/screenshot blobs or private checkpoint refs. Keep the original audit repo private; obtain approval for any rename/new-repository/publication operation. | Refreshed 0.2.21 one-commit/no-remote handoff verifies exact tree equality and excludes all 30 earlier image blobs. Owner approved a fresh public source repository, vellora-windows, on October 6. Existing audit history remains private; installer releases remain deferred. |

## P1: public beta installer gates

| ID | Task | Acceptance criteria |
| --- | --- | --- |
| REL-01 | Clean Windows install/upgrade/uninstall | Fresh Windows 10/11 account without development tools; WebView2, tray, second launch, shortcut, startup on/off, upgrade and uninstall tested; actual retention recorded. October 6 existing-account update/version/payload/notices/startup checks passed; this fresh-account gate remains open. |
| RUN-01 | Real microphone/failure testing | Hold/lock/stop, sleep/resume, permission/no-device, disconnect, five-minute limit, offline/auth/rate-limit errors; no stuck busy/Recording state or unbounded allocation. Use dummy speech. Owner confirmed the original partial-modifier reproduction fixed on October 6; the broader gate remains open. |
| RUN-02 | End-to-end OpenAI compatibility/accuracy | Small consented dummy clips for each model and cleanup; technical-word accuracy, latency and billing comparison. Audit used no paid calls; review model lifecycle/costs before release. |
| CLIP-01 | Clipboard/paste hardening | Preserve all supported formats or explicitly label/limit preservation; test rich HTML/image plus text, empty clipboard, concurrent updates, slow/elevated destinations and both top-level and same-window focus changes. Current preservation is plain text, delayed and best effort. |
| LIC-01 | Redistribution and artwork review | Verify all exact-version license/notice texts and alternative/MPL/Unicode/data obligations; required notices included in installer. Confirm rights to all branding/assets and document provenance. |
| ART-01 | Current artwork provenance recorded; isolate historical assets | New text-generated logo and derived Windows icons replace current assets; prompts/inputs/hashes in ARTWORK_PROVENANCE.md. Publish from the clean-history candidate rather than exposing the old private artwork history; no exclusivity/trademark guarantee is inferred. |
| REL-02 | Signing and release process | Choose signing approach; SHA-256 checksums, release notes/platform requirements; test SmartScreen. Publish only after explicit public-release authorization. |
| PRIV-01 | Complete local-data removal flow | Confirmation addresses active text/history/memory, legacy backups/credentials, crash leftovers, settings backups and clipboard limits; test isolated data. |
| GUI-01 | Real accessibility/display coverage | Keyboard/screen-reader on Windows; 100/125/150/200% DPI, long transcripts, high contrast, reduced motion and two monitors; no clipped actions/focus traps in either theme. |

## P2: next beta

| ID | Task | Acceptance criteria |
| --- | --- | --- |
| GUI-02 | Microphone picker and live local test | Simple setup/key/hotkey/focus guidance and default-device availability check are implemented. Still add persisted input-device choice and a free local meter that verifies sound capture, with permission/device error feedback. |
| HIST-01 | Search/pagination/export/retention | Records beyond newest 100, bounded parameterized queries/dashboard aggregation, selected-text export and optional expiry. |
| COST-01 | Better cost accounting | Version prices, include failed/retried requests and observed token usage; separate estimates from provider bills. |
| ARCH-01 | Testable pipeline orchestration | Extract main.rs runtime/delivery into audio/API/clipboard adapters; integration failure tests preserve single-owner stop semantics. |
| DEP-01 | Upstream maintenance monitoring | Track remaining transitive unmaintained/platform advisories; compatible Tauri updates without blanket warning suppression. |
| MODEL-02 | Speaker-label output | Parse diarized_json segments and render speaker boundaries, or retain explicit plain-text limitation; test long/conversation samples. |
| MODEL-01 | Additional/offline providers for Windows | Consented fixtures compare accuracy, latency, privacy and full-session cost before adding engines; no other operating-system support is planned in this phase. |

## Dated provider maintenance

| ID | Task | Acceptance criteria |
| --- | --- | --- |
| MODEL-03 | Replace retiring cleanup snapshot before December 11, 2026 | Review/test by November 20; preserve user choice, validate account/API/quality/retries, migrate settings/history safely and update exact pricing/support fields. See [MODEL_LIFECYCLE.md](MODEL_LIFECYCLE.md). |
| MODEL-04 | Replace retiring transcription models before February 26, 2027 | Review/test by February 1; completed-file uploads can remain, but model enums/language hints/response parsing/access/pricing all need verified migration. No live accuracy claim without dummy-clip evidence. |

## Before changing repository visibility

Rerun PUB-01/PUB-02/PUB-03, review this backlog and [audit](AUDIT.md), confirm the clean-history and license/source obligations, set suitable private security reporting/contact, and obtain a separate public-opening decision. Keep the original repository private as an audit archive. If instead changing an existing repository's visibility, historical artwork rights and unapproved Actions installer artifacts must be resolved first. Review artifact uploads are private-only; source approval does not authorize a public installer. An unsigned/unverified installer must not be presented as production-ready.
