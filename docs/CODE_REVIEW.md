# Source review for publication approval

> **Source publication update, October 6, 2026:** the owner approved a fresh
> public Windows source beta. This document retains the dated private
> preparation record; historical CI links refer to the private audit archive.
> See [current publication status](PUBLICATION_STATUS.md). Public installer
> distribution remains deferred.

Reviewed October 6, 2026 for the 0.2.21 candidate, retaining the October 4
review and its historical verification results. This review covers the Windows Rust recording,
transcription, cleanup, clipboard, history, credentials, settings and help
commands, plus the React IPC, settings, onboarding, diagnostic and transcript
controls. It is a source and regression review, not a penetration test or a
claim that a fresh Windows installation has been certified.

The reviewed design is reasonable for a small Windows beta: a local UI and
backend, the user's own OpenAI account, no Vellora-hosted transcript account,
and optional local text history. No newly confirmed critical exploit or
undisclosed transcript analytics/upload service was found in this scope.
The candidate includes the concurrent state corrections below. The previously
built 0.2.19 installer does not contain these fixes; an older installer is not
evidence that the October 6 shortcut corrections have been validated natively.

## Findings addressed in this review

| Priority | Concrete trigger and impact | Candidate correction |
| --- | --- | --- |
| Medium | A microphone driver takes longer than the startup timeout. Repeated retries could create additional detached workers. A late old-worker error could be attributed to a newer session. | The recorder holds one worker guard until actual worker exit, checks cancellation before continuing device setup/play, and attaches the originating session ID to each error/limit event. Main accepts only events owned by the current session. |
| Medium | An older frontend refresh finishes after a newer refresh/event. It could show old settings/status or bring deleted text back into the UI. | Request/resource revisions prevent stale refresh results from replacing newer state. |
| Medium | Turn history off, navigate before saving finishes, then change model/appearance. Separate views previously saved stale whole settings objects and could turn history back on. | A shared serialized settings queue applies only each user's changed fields to the latest acknowledged settings. |
| Low | Transcript A's delayed-paste countdown remains active when the latest controls are reused for transcript B. The timer still refers to A. | Cancel and reset pending controls when the transcript ID changes. |
| Medium | Leave a recording running without keyboard events for over 30 seconds. The old stale-input reset discarded its lock/hold state, so a stop could be ignored. A failed start could also leave a phantom hotkey recording state. | Stale-input recovery preserves locked and physically held recordings and recovers a missing release once. Failed accepted starts reset the hotkey recording state. Focused state-machine tests cover recovery cases. |
| Medium | Release and quickly re-press only Ctrl or only Win while the other modifier stays held. The partial re-press was treated as a double tap and could unexpectedly lock recording. | A double tap now requires both modifiers to have been released between presses. Partial re-presses resume the existing hold rather than switching to locked mode; a slow full release does not extend the original double-tap deadline. |
| Medium | Resume a hold before its delayed stop fires, or release it again while the first timer is still waiting. The older timer could stop the resumed recording or shorten the newer release's grace period. | Each session tracks its pending release token. Resuming a hold or upgrading to locked mode clears it; another release gets a new token. A delayed stop must atomically match both the session ID and the current pending release token before claiming the session. |
| Medium | A hold/lock notification is queued during blocked startup or processing, then recording is reset before the worker handles it. Replaying the old notification could start an unintended recording after busy clears; an old failure could also reset a newer gesture. | Notifications carry the hotkey reset epoch. The worker checks it again when reserving a session, including after the credential read, and mode changes require a matching session epoch. Rejection resets/drains queued gestures under the enqueue mutex; older failures cannot reset or drain a newer epoch. |

The recording changes are in [audio.rs](../src-tauri/src/audio.rs) and
[main.rs](../src-tauri/src/main.rs). Frontend updates are in
[App.tsx](../src/App.tsx) and its settings/transcript views. Keyboard recovery is
in [hotkey.rs](../src-tauri/src/hotkey.rs).

The October 6 shortcut changes preserve the existing 450 ms release grace
period and deliberate double-tap behavior. Per-session release identity prevents
an expired timer from owning a resumed/replacement recording. Epoch checks also
prevent notifications that were queued before a reset from committing a new
session afterward. Device setup and credential operations run outside the
hotkey mutex; only brief state checks/reservations use that guard.

The audio suite additionally covers a synthetic driver failure after the worker
has reported ready. It verifies one error event with the originating session ID,
inactive capture, consumption of the failed recording on stop, and release of
the worker guard. This adds regression evidence for the existing error path; it
does not reproduce a physical microphone driver failure.

## Privacy and security boundaries inspected

- [credentials.rs](../src-tauri/src/credentials.rs) stores the key in Windows
  Credential Manager. IPC exposes save/remove/presence, never a key getter.
  Explicit removal has a migration marker so the old key is not reimported.
- [openai.rs](../src-tauri/src/openai.rs) uses fixed OpenAI HTTPS endpoints,
  rejects redirects, has bounded request/retry deadlines, and omits remote
  response bodies from errors. Cleanup treats the transcript as input data,
  requests `store: false`, and falls back to raw text if it fails.
- [audio.rs](../src-tauri/src/audio.rs) limits capture by duration and sample
  count. Random temporary WAV files use RAII deletion; startup cleanup is
  restricted to owned, regular, sufficiently old files and avoids reparse points.
- [main.rs](../src-tauri/src/main.rs) guards sensitive commands and transcript
  events for the main window. The overlay has no transcript preview.
- [support.rs](../src-tauri/src/support.rs) opens only fixed help destinations.
  Its local microphone check has its own worker guard and opens no audio stream.
  Support data and the frontend formatter explicitly allowlist nonsecret fields;
  paths, device names, transcript text, keys and freeform errors are excluded.
- Production [Tauri configuration](../src-tauri/tauri.conf.json) restricts the
  bundled frontend with CSP and only event-listening capabilities. No broad
  frontend filesystem/shell permission is granted.
- History queries/deletions are parameterized and SQLite secure deletion is
  enabled. Settings are validated and written through a synced staging file.
- Reviewed React code renders untrusted strings as escaped JSX text. No remote
  frontend content, `eval` or HTML injection API was found. Browser storage holds
  the nonsecret setup preference, not transcripts or keys.

Detailed retention, legacy backup and provider boundaries remain in
[PRIVACY.md](PRIVACY.md). This review does not certify the separate repository
history, screenshots, dependency licensing or artifact scans.

## Limits that still matter before public binaries

1. Auto-paste checks the top-level destination window, not the browser tab or
   field. Input injection does not confirm that the destination accepted text.
   Clipboard preservation supports plain text/empty state and is best effort;
   rich formats, delayed consumers and concurrent changes need native testing.
2. History is unencrypted. GUI deletion cannot erase legacy backups, provider
   records, clipboard history, filesystem snapshots or forensic SSD remnants.
   A current operation takes a settings snapshot when recording stops; changing
   retention/delivery settings or clearing history during processing does not
   cancel that operation. Finish or quit active dictation before removing data.
3. A blocked Windows driver cannot be forcibly cancelled safely. The new guard
   bounds the recorder to one worker, but restart may be needed if it never
   returns. Synthetic tests prove guard and session ownership, not driver quality.
4. Real microphones, sleep/resume, permission denial, elevated/slow destinations,
   fresh-account installation, broader upgrade scenarios, uninstall, high DPI
   and screen readers remain native
   release checks. No paid OpenAI request was made in this review.
5. Runtime orchestration remains concentrated in `main.rs`; recording/API/
   clipboard adapters would make future failure integration tests easier.
   Dashboard totals inspect at most 10,000 rows, history loads the newest 100,
   and cost figures estimate saved successful work rather than actual billing.

These are concrete beta limitations, not a reason to add other operating
systems or rewrite the whole app before reviewing its source publication.
See [ROADMAP.md](ROADMAP.md) for release acceptance criteria.

## Verification

### October 4: prior review

The focused audio suite passed eight tests, including a blocked synthetic worker
that times out, rejects retries until exit, then permits restart, and a late
old-session error that cannot claim a newer/missing session. Fixtures use
channels and private temporary data, without opening a microphone or making an
API request. Formatting and whitespace checks passed.

`npm run check:frontend` passed ESLint with zero warnings, both TypeScript
projects, and 39 behavioral tests in six files. Eleven new cases cover refreshes
finishing in reverse order, stale failures after newer successful edits,
settings/status/health events arriving during reads, deletion followed by a
state change, queued privacy plus model/theme edits, continued edits after a
failed save, and replacing a transcript during a paste countdown. The main UI
also shows unavailable key status honestly and receives locked recording mode
from the existing backend event.

The October 4 complete Rust suite passed all 44 tests, including 13
shortcut-state tests. Rust formatting and Clippy with warnings denied also
passed. Together with the frontend suite, that candidate had 83 passing
behavioral tests. Its production frontend asset build passed; no local release
executable, installer or installation was produced by that source-review task.
Hosted installer checks
are recorded separately in the [publication review](PUBLICATION_REVIEW.md).

### October 6: 0.2.21 shortcut candidate

The complete local Rust suite passed **62 tests**, including **21 hotkey
state-machine tests**, **nine recording-session tests**, and **nine audio
tests**. The audio count increased from eight with the post-ready worker-failure
case. Rust formatting and Clippy across all targets with warnings denied passed.

`npm run check:frontend` passed ESLint with zero warnings, both TypeScript
projects, and all **39 frontend behavioral tests** on October 6.
Together, the current local suites have **101 passing behavioral tests**. The
new shortcut/session cases cover partial modifier re-presses, deliberate full
release/double taps, resumed holds, old release timers, queued starts invalidated
by reset, and failures that must not disturb newer gestures.

These are synthetic state-machine, worker, session, and frontend checks. They
make no paid OpenAI requests and do not validate physical microphone capture,
native keyboard-hook behavior, sleep/resume, clipboard delivery, or installation.
The production frontend asset build passed. The hosted **Test rendered GUI with
synthetic data** step also passed for source commit
`62b9297390e354e75e4147a7a7b86c8cf2b1b030` in
[Windows CI run 37471723603](https://github.com/Felix561/vellora/actions/runs/37471723603/job/112296641524)
on October 6, from 13:34:48 to 13:35:28 UTC. The complete run subsequently passed
all three jobs, including 62 Rust tests, dependency audits, full-history scanning
and actual release/installer verification. The owner-approved existing-account
update also passed version/payload/notice/startup checks. Exact artifact evidence
and the separate native gates are in [PUBLICATION_REVIEW.md](PUBLICATION_REVIEW.md).

After that update, the owner reported that the original Ctrl release/re-press
while Win remains held now works. This confirms the reported physical sequence
on the owner's PC; it does not close the broader native hardware/API checks.

That rendered run checked both themes and all five main pages, plus all four
setup steps. Selected CDP Tab/Shift+Tab/ArrowRight/Home transitions verified
actual Chromium focus and visible outlines. Approximately 16,000 characters of
synthetic multiline/unbroken/Unicode/literal-HTML transcript text stayed literal
and untruncated, without horizontal overflow and with reachable actions. Forced
colors, the actual React overlay's reduced-motion behavior with mocked Tauri
events, and 125/150/200 percent viewport/device-pixel-ratio reflow checks passed.
Error/persistence feedback, privacy/support/setup handling, confirmations,
recovery and paste cancellation also passed, with no unhandled promises or
external network attempts. See [GUI_TESTING.md](GUI_TESTING.md) for precise
coverage and the two corrected harness issues.

Rendered emulation does not close the real browser-zoom, Windows DPI,
screen-reader, complete keyboard-traversal, native shortcut, physical microphone,
clipboard, sleep/resume, or installer gates. Earlier CI runs and the October 4
results are separate evidence and do not substitute for these remaining checks.

The first hosted attempt identified a high-severity indexed-source-map denial
of service in the development dependency `source-map-js` 1.2.1. It was cancelled
before Rust or installer compilation. The lockfile now selects 1.2.2, within
the existing parent ranges; all other dependency versions are unchanged.
`npm audit` then reported zero vulnerabilities. The runtime notice inventory
regenerated unchanged with 236 texts and zero unresolved package notices.
See the [upstream advisory](https://github.com/advisories/GHSA-68fv-2mgg-jv7q)
for the affected and patched versions. The dependency is used by Vite/PostCSS
and jsdom/css-tree during development and testing.
