# Publication audit

> **Source publication update, October 6, 2026:** the owner approved a fresh
> public Windows source beta. This document retains the dated private
> preparation record; historical CI links refer to the private audit archive.
> See [current publication status](PUBLICATION_STATUS.md). Public installer
> distribution remains deferred.

Reviewed October 1, 2026 for Vellora 0.2.18. Scope: React/TypeScript GUI, Tauri configuration/commands, Rust audio/API/clipboard/history/settings/hotkey pipeline, dependencies, Git history, licensing, documentation and private repository preparation.

The source is suitable for a private development handoff after validation below. A public installer still needs the gates in [ROADMAP.md](ROADMAP.md). This is a source review and targeted regression audit, not an independent penetration test, legal certification or clean-machine product certification.

## October 2 update: 0.2.19 Windows polish

Added a user-first README, Windows troubleshooting, changelog and issue forms,
Help & About, and a reopenable four-step setup guide. Setup waits for confirmed
key status, saves a skip/completion preference, and does not initiate recording
or paid API calls. Its microphone check reads the default device configuration;
it does not open an audio stream or validate sound capture or permissions.

Support information uses explicit enum/boolean fields on both sides of the IPC
boundary. It excludes keys, transcript text, free-form errors, device names,
usernames and file paths. External help opens only fixed, main-window-guarded
destinations, without granting a general shell or URL-opening capability.

Validation: 28 frontend regression tests, frontend lint/type checks and production
asset build passed; 35 Rust tests and Clippy passed. npm audit found zero
vulnerabilities; RustSec returned no vulnerability findings, with the previously
documented seven maintenance/platform warnings. Third-party notices regenerate
with zero unresolved upstream texts. Production npm dependencies are unchanged.

Built one local Windows x64 executable and NSIS installer for 0.2.19. The existing
installed app was left running; the review installer was not installed or
represented as a verified clean-machine release. The staged secret scan passed.

New frontend tests cover persistence failures, startup preferences, theme saving,
deletion confirmations, recovery without history, delayed-paste cancellation,
setup entry/skip/completion/reopening, focus after key saving, clipboard fallback
and injected diagnostic values. They use synthetic data and make no OpenAI calls.
The optional interactive browser smoke script now includes Help and setup but
was not rerun: the debugger-based browser launch was blocked and managed browser
automation was unavailable. A screenshot-only renderer without a debugger later
produced fresh 1240 x 1000 previews of all seven views in both appearances. Each
was visually reviewed and added to the README with synthetic transcripts,
statistics, cost estimates and saved-key status. No installed application data
was used. Native Windows behavior, real DPI/accessibility checks and live API
testing remain separate public release gates. The repository stays private.

## October 4 update: final private publication review

Prepared Vellora 0.2.20 with a revised README, pinned peer comparisons,
publication/privacy/code/licensing/model-lifecycle reports and proposed public
copy. Repository visibility remains private; author credit stays Felix Seitzer
as explicitly approved. At the owner's later request, a new logo was generated
with recorded text prompts/inputs/hashes, replacing all current branding and
Windows icon assets. All sixteen synthetic screenshot files were refreshed and
visually/metadata reviewed. Earlier artwork remains unverified in private
history; the publication plan uses an independent clean source snapshot.

The current code corrects bounded microphone startup/event ownership, stale
shortcut recovery, failed-start shortcut state, asynchronous UI/settings races
and transcript-specific paste countdowns. Frontend lint/type checks and 39 tests
in six files passed. Rust formatting/Clippy and all 44 tests passed. The
production frontend build passed. npm and RustSec audits reported zero
vulnerabilities; the documented maintenance/platform warnings remain. Notice
regeneration reports 236 distinct texts and zero unresolved package notices.

The publication privacy review covered 43 baseline reachable commits/313 blobs,
all historical images, repository PR/comments metadata, and 42 available Actions
log archives. It found no accidental owner path, personal email, real key or
personal dictation data in the inspected source/history or available repository
surfaces. Intentional credits, upstream notice contacts and invented fixtures
were classified explicitly. New staged/final-history scans remain part of the
commit/push checks; full scope and limits are in PUBLICATION_PRIVACY.md.

The old local 0.2.19 executable failed: compiled dependency/source locations
contain personal Windows build roots. Its compressed installer is not approved
for publication. New hosted-build automation remaps compiler roots, scans the
unpacked release EXE, verifies installer payload and all three license/notice
resources by hash, creates checksums and prepares exact covered-source archives.
Actual hosted-build/payload results are recorded in the final run report rather
than inferred from source checks. No local release executable/installer is built
or installed during this preparation.

Native clean-account/hardware/clipboard/accessibility checks remain open. Current
dictations use options captured at recording stop; deletion/retention guidance
now explains the active-operation boundary. Provider retirement dates are
tracked in MODEL_LIFECYCLE.md. Unapproved Actions installer artifacts must be
removed/expired before source-only visibility changes, and review uploads are
conditioned on private repository visibility. Public opening and public installer
release each require the owner's explicit final approval.

## October 5 completion: private review candidate

[Hosted run 37279517885](https://github.com/Felix561/vellora/actions/runs/37279517885)
passed all three jobs for application candidate
`7419e7eee8ecf39601e3feaa1e69bf9069921d05`: 39 frontend and 44 Rust regressions,
lint/type/format/Clippy checks, dependency audits, regenerated notices and
full-history secret scanning. npm/RustSec reported no vulnerabilities; the
existing maintenance/platform warnings remain recorded below.

The standalone EXE and actual extracted installer payload each passed all five
private-path categories with zero findings. Installer verification now accounts
for the pinned Tauri CLI 2.11.2 `UNK` to `NSS` bundle marker: it compares every
payload byte to that exact expected transformation, directly scans the payload,
and verifies all three bundled notice hashes. Earlier failures exposed narrow
scanner/verifier assumptions and blocked uploads; no check was disabled to pass.
The successful candidate has private Windows and exact-source companion
artifacts, with a 14-day review lifetime. Hashes are in PUBLICATION_PRIVACY.md.

The October 5 GitHub follow-up found no new sensitive text or unexpected
repository-surface content. All six additional completed-run logs (1,080,083
bytes) and the successful final run log (329,592 bytes) passed the redacted
privacy checks. A failed/cancelled run remains a failed/cancelled validation run.

The final handoff prepares a one-commit local source repository and ZIP with no
remote, exact tree equality and no matches to all 30 earlier image blobs,
including coding-tool checkpoints. Final documentation and four surplus EOF
blank-line removals are a separate non-behavioral follow-up; type checks and
handoff checks are repeated. No local release EXE/installer is built or installed.
Native runtime gates and the owner's source/installer approvals remain open.

## October 6 follow-up: shortcut reliability

The reported Ctrl release/re-press while Win remains held could accidentally
enter locked recording. A lock now requires both modifier groups to be released
between taps. Resuming a held recording cancels its own release timer; session
and release tokens prevent older timers from claiming a newer release/session.

Independent review also found queued shortcut starts that could survive a
microphone startup failure or an automatic stop. Reset epochs now invalidate
those notifications, with a second check when committing the session after
credential lookup. Slow microphone/app operations remain outside the keyboard
mutex. Added regressions cover the reported modifier sequence, queued resets,
timer ownership and a microphone worker that becomes ready and then fails.

Local verification passed 62 Rust and 39 frontend tests, formatting, Clippy,
ESLint, both TypeScript projects and the production frontend asset build. The
first hosted attempt exposed the newly reviewed source-map-js advisory; the
development lock entry was updated from 1.2.1 to patched 1.2.2, with all other
dependency versions unchanged. npm audit then returned zero vulnerabilities;
notice regeneration remained at 236 texts and zero unresolved package notices.
The [upstream advisory](https://github.com/advisories/GHSA-68fv-2mgg-jv7q)
records the affected and patched versions.

All three jobs in [hosted run 37471723603](https://github.com/Felix561/vellora/actions/runs/37471723603)
passed for `62b9297390e354e75e4147a7a7b86c8cf2b1b030`, including 101 behavioral
tests and the expanded rendered GUI checks. The standalone EXE and actual
installer payload passed all five private-path categories, exact reviewed NSIS
marker comparison and all three notice hashes. The final run log passed the
redacted secret/owner-path checks. An owner-approved update from 0.2.18 to 0.2.21
was performed once; installed bytes/version/notices and startup preservation
were verified, then the app restarted. Full evidence is in the publication
review. The owner then confirmed the originally reported partial-modifier
shortcut sequence now works on the installed update. Broader native
microphone/clipboard, fresh-account install/uninstall, screen-reader, real DPI
and multi-monitor release gates remain open. No paid API calls or personal
dictation fixtures are used by these checks; repository visibility stays private.

## Addressed findings

| Severity | Finding and impact | Correction |
| --- | --- | --- |
| High | Disabled production CSP and broad frontend capabilities | Bundled-asset CSP, event-listen/unlisten only, sensitive Rust commands/events main-window-only; overlay no transcript preview. |
| High | Known npm/Rust dependency vulnerabilities | Compatible locked upgrades; zero npm and RustSec vulnerability findings after updates. Maintenance/platform warnings tracked separately. |
| High | Unbounded audio, stderr-only capture failures and incomplete failure cleanup | Startup handshake/GUI diagnostics, five-minute/sample/file caps, per-capture cancellation, RAII WAV deletion and guarded stale-file cleanup. |
| High | Successful transcription lost on clipboard failure; autoPaste ignored | Honor settings, preserve latest recoverable text, show delivery failures, support Copy without persisted history. |
| Medium | Duplicate stop/stale reset events interfering with other operations | Expected-session atomic claims, one busy-release owner, generation-bound idle timers, ignore audio levels outside Recording. |
| Medium | Deleted API key re-imported on restart | Durable one-time credential-import marker for migration and explicit save/removal; legacy backup documented. |
| Medium | Cleanup inspected only first output item | Parse all typed assistant text after reasoning, reject incomplete/empty output, keep raw fallback; separate instructions/data, store:false. |
| Medium | Remote error bodies exposed in diagnostics; uncertain retry budgets | HTTPS-only/no redirects, sanitized status/request ID, overall timeouts and one transient retry; return quota/rate errors for user action. |
| Medium | Direct settings writes and startup failure on invalid files | Size/validation checks, atomic synced persistence, preserved invalid-file backup/default recovery and visible warning. |
| Medium | Keyboard-hook installation failure reported success | Initialization handshake and degraded-health feedback; automatic stop resets hotkey lock. |
| Medium | No text deletion controls; recovery broken with history off | Parameterized delete/clear, secure-delete SQLite setting, confirmation and matching in-memory deletion. |
| Medium | GUI re-paste targeted Vellora's own window | Three-second cancellable focus countdown, one pending action, navigation cancellation and target/error checks. |
| Low | Uncaught/optimistic API-key/model/history operations and missed health | Honest persistence feedback, initial health query, persistent dismissible dictation failures. |
| Low | Publication license/CI missing, unsafe TLS bypass guidance | MIT, public-facing docs, pinned CI/Dependabot, removed certificate bypass instructions and Cargo revocation override. |

Earlier 0.2.16/0.2.17 changes added full native indicator click-through, GUI startup control and persisted Notebook/Windows Classic appearances.

## GUI assessment

Both appearances are coherent enough for a beta. Notebook gives a distinct identity; Classic uses consistent raised controls/title/status/navigation. Reliability and onboarding matter more than a broad redesign now.

Changes improve helper-text contrast, reduced motion, keyboard semantics, confirmation focus, visible failures, long model layouts and small-window scrolling. Synthetic tests cover Dashboard/History/Models/Settings in both themes at 1040, 880 and 520 pixel widths. Native minimum size remains 880 by 620; 520 is a layout stress test, not a supported native size. Synthetic screenshots contain no personal text.

Remaining: real Windows screen-reader/high-contrast/DPI checks, microphone selection/onboarding, search/pagination and complete-data deletion. See [GUI_TESTING.md](GUI_TESTING.md) for reproducible smoke checks and limits.

## October 1 validation baseline (0.2.18)

No paid OpenAI call or destructive action on personal history was used. Automated tests use isolated data.

- TypeScript/Vite production build and mocked GUI smoke pass.
- Rust formatting and Clippy with warnings denied passed. All 33 tests passed on a freshly built executable after repairing cold-start-sensitive HTTP fixtures. Final verification disabled only incremental compilation to avoid local Windows cache-access warnings; security checks were unchanged.
- Regression coverage includes cleanup ordering/completeness, privacy/retry/timeout policy, bounded audio/WAV cleanup, stale-file safeguards, settings recovery/validation, history deletion, appearance compatibility, costs and hotkey transitions.
- npm audit: zero vulnerabilities including build tooling.
- RustSec: zero vulnerabilities in 608 locked crates against official advisory DB commit `3461c0d8f85d084552dd999c58d97c7123a9e0fd` (1,278 advisories).
- Gitleaks 8.30.1 redacted scan of 38 existing commits: no leaks. New changes/final history scanned before push.
- Actions pinned to official-repository commits; Gitleaks downloads checksum verified; no API key needed in CI.

The 0.2.18 NSIS installer built and upgraded the existing installation successfully. Installed license/notice resources match source hashes. Native checks confirmed ready health, both themes synchronizing with the overlay, persisted settings/startup reads, every page fitting the current viewport, sensitive overlay commands denied, frontend event emission denied, inline scripts blocked by production CSP, no overlay transcript, and visible click-through/no-activate/layered indicator flags. Existing preferences were restored after theme checks. These checks used no paid API calls. Exact-commit CI status is available in the repository Actions tab. Mock tests do not prove hardware recording, destination input acceptance, billing or installer behavior on another PC.

## Remaining risks and decisions

1. **Clipboard is plain-text preservation only.** Pure non-text content skips auto-paste; sequence checking avoids overwriting newer updates. Rich formats and slow consumers still need CLIP-01. The target guard checks the top-level window, not a browser tab or focused field within it. SendInput success is delivery of a shortcut, not confirmation the target accepted it.
2. **Unencrypted local history and scoped deletion.** OpenFlow backups, old temporary audio, clipboard/provider records and filesystem backups remain outside GUI deletion. [Privacy details](PRIVACY.md).
3. **Incomplete hardware/API/clean-machine coverage.** Dummy-speech testing on a fresh Windows account is a public-installer gate. No live accuracy claim is made. Diarize uses plain JSON/text output; speaker-label output is not implemented, and the GUI now states that limitation.
4. **Maintenance warnings and redistribution review.** Zero known vulnerabilities does not mean no risks. Exact upstream notices are inventoried, but license choices/asset rights still require review. Five UNIC crates in the Windows graph are unmaintained (RUSTSEC-2025-0081, -0075, -0080, -0100, -0098). proc-macro-error (RUSTSEC-2024-0370) and glib (RUSTSEC-2024-0429) affect non-Windows lock entries. No blanket advisory ignores were added. [Third-party notices](../THIRD_PARTY_NOTICES.md).
5. **Cost charts undercount usage.** Session duration includes microphone initialization and cleanup tokens are approximate. Successful saved history excludes failed/retried/deleted/unsaved work. GPT-4o mini Transcribe remains the lowest listed file-transcription estimate, approximately $0.003/minute at review time. Recheck [OpenAI pricing](https://developers.openai.com/api/docs/pricing) before release.
6. **Maintainability/performance.** Runtime orchestration remains concentrated in main.rs/global mutex state; integration failure tests need injected adapters. History/dashboard need large-dataset testing and bounded pagination.

## Primary research

- [Tauri capabilities](https://v2.tauri.app/security/capabilities/), [CSP](https://v2.tauri.app/security/csp/), [Windows prerequisites](https://v2.tauri.app/start/prerequisites/#windows).
- OpenAI [speech-to-text](https://developers.openai.com/api/docs/guides/speech-to-text), [Responses](https://developers.openai.com/api/reference/cli/resources/responses/methods/create), [rate limits](https://developers.openai.com/api/docs/guides/rate-limits), [data controls](https://developers.openai.com/api/docs/guides/your-data), [pricing](https://developers.openai.com/api/docs/pricing).
- Windows [foreground window](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getforegroundwindow), [clipboard sequence number](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getclipboardsequencenumber).
- GitHub [repository creation](https://docs.github.com/en/repositories/creating-and-managing-repositories/creating-a-new-repository), [secret scanning](https://docs.github.com/en/code-security/concepts/secret-security/secret-scanning), [Gitleaks](https://github.com/gitleaks/gitleaks), [RustSec](https://rustsec.org/).

The roadmap converts unresolved findings into tasks with acceptance criteria. Repository visibility stays private until a separate public-opening decision.
