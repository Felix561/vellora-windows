# Final publication review

Prepared October 4 and updated October 6, 2026 for the Vellora 0.2.21 private review candidate.
**October 6 publication decision:** the owner approved publishing the reviewed
clean-history source in [Felix561/vellora-windows](https://github.com/Felix561/vellora-windows).
The development/audit repository remains private. Public installers, release
tags and announcements remain deferred. This document preserves the dated
preparation evidence; see [current publication status](PUBLICATION_STATUS.md).
Felix Seitzer remains the copyright credit, as requested by the owner.

## Recommendation

Publishing a focused Windows source beta is a reasonable next step once the
remaining ownership and publication decisions below are resolved. Vellora has
a clear purpose: OpenAI dictation with a personal API key, recoverable text,
local history, a click-through indicator, and two appearances. Its Rust/Tauri
architecture is a suitable foundation. This review does not establish
production readiness or native-device compatibility on every Windows machine.

The GUI is presentable for a Windows beta: its two appearances, guided setup,
Help view and recovery controls give the project a coherent entry point. The
fourteen refreshed synthetic screenshots show the actual 0.2.20 frontend with
the newly generated artwork. The 0.2.20 changes also improve state/error
behavior. Native screen-reader,
display scaling and multi-monitor checks remain open.

Source availability and installer distribution require separate approval.
The existing local 0.2.19 installer must not be the public download: its embedded
executable exposes developer build paths and lacks the new fixes/native notices.
The replacement is a private, locked hosted build with compiler path remapping,
binary inspection, installer payload/resource verification and checksums.

## Review package

| Material | What to review |
| --- | --- |
| [README](../README.md) and [publication copy](PUBLICATION_COPY.md) | Current presentation and the exact proposed text after source approval; all 14 examples remain synthetic |
| [Project comparison](PROJECT_COMPARISON.md) | Pinned evidence from OpenWhispr and Handy, plus the commercial Wispr Flow distinction |
| [Code review](CODE_REVIEW.md) | Concrete corrections, regression coverage and remaining runtime limits |
| [Publication privacy](PUBLICATION_PRIVACY.md) | Source/history/images, intentional credits, repository surfaces and old binary finding |
| [Licensing review](LICENSING_REVIEW.md) | MIT, dependency/native component conditions, AI-created code, recorded replacement-artwork provenance and historical asset-rights limits |
| [Main notices](../THIRD_PARTY_NOTICES.md) and [native notices](../THIRD_PARTY_NOTICES_NATIVE.md) | Exact-version license texts and distribution materials |
| [Privacy](PRIVACY.md), [troubleshooting](TROUBLESHOOTING.md) and [roadmap](ROADMAP.md) | Actual cloud/local behavior and remaining manual release checks |

## What changed during preparation

- Reworked the README around the Windows dictation workflow, setup, screenshots,
  cloud/API billing and concise limitations. Prepared public wording separately
  without asserting that the private repository or downloads are public.
- Added an independently researched peer comparison. No peer implementation,
  branding, logo, screenshot or slogan was copied.
- Bounded microphone startup workers until they actually exit; tagged capture
  events with their originating session to reject late events from an old worker.
- Repaired stale shortcut recovery while preserving locked and genuinely held
  recordings, and cleared phantom shortcut state after failed accepted starts.
- Serialized partial settings updates and protected asynchronous reads from
  overwriting newer state. Transcript-specific paste countdowns are cancelled
  when the displayed transcript changes.
- Added targeted synthetic regressions for these failure sequences.
- Added compiler path remapping and count-only release binary privacy checks.
  Installer review scans the extracted payload directly, validates it against
  the expected Tauri NSIS-marked variant of the inspected EXE, verifies all three
  notice resources, and records SHA-256 checksums.
- Added external NSIS/plugin/WebView2 loader notices and replaced the earlier
  unverified artwork with a newly generated logo, Windows icons and refreshed
  screenshots. Exact prompts, inputs and hashes are documented.

## Privacy assessment

The baseline review covered 43 reachable local commits and 313 blobs, including
commit identities, historical images, and files removed from the current tree.
No accidental personal email, directory, real API key, recording or transcript
was identified in that reviewed source/history. Main history uses GitHub noreply
email identities. Local coding-tool checkpoints were also checked and are not
intended publication branches. Do not publish them with a mirror push.

The approved author name and repository handle are intentional public credits.
Third-party notice contacts and invented privacy-test paths are intentional
license/test material. Installed credentials, personal history, clipboard and
microphone data were not used for this audit. Pattern/image review has limits;
the evidence and exclusions are in the privacy report. Newly staged files and
the final committed history are scanned again before the private push.

The old local executable failed the binary privacy check. A successful scan of
source cannot clear a compiled artifact, and a plain-string scan of a compressed
installer cannot clear its payload. Only the verified new private artifact is a
candidate for a later release decision.

## Legal and asset decisions

A Windows `.exe` is a normal way to distribute an open-source application. MIT
permits redistribution of the software subject to retaining its notice; it does
not relicense dependencies, Microsoft Windows, WebView2 or OpenAI services.
Code signing is a separate publisher/integrity decision. See the primary-source
license matrix for exact conditions rather than treating the executable format
as an obstacle.

The owner reports creating Vellora solely with AI agents. That does not by
itself prohibit publication or prove that every generated element is exclusively
copyrightable or free of third-party rights. MIT applies to rights actually held;
the licensing review explains the current OpenAI terms and German copyright
guidance without making a legal certification.

**Current artwork provenance recorded:** at the owner's request, a new logo
was generated with Codex's built-in OpenAI image tool from a text-only prompt.
No earlier logo or external reference image was supplied. A paper variant used
only that new master as input; Windows icons were resized with Tauri's icon CLI.
The [artwork record](ARTWORK_PROVENANCE.md) contains both prompts and asset hashes.
This establishes the recorded origin, not exclusive copyright, trademark
clearance or a universal non-infringement guarantee.

**Historical distribution decision:** earlier commits and old screenshots
still contain the previous unverified artwork. The recommended publication
candidate is an independent clean-history source snapshot. Preserve the existing
repository as a private audit archive; do not simply make it public or publish
an orphan branch alongside its existing refs. A future rename/new-repository
operation remains subject to approval. No history rewrite or force push was made.

## Approval choices

| Decision | Current status | Required action before it happens |
| --- | --- | --- |
| Retain Felix Seitzer credit | Approved by the owner | Keep the existing credit; no Git identity rewriting is proposed |
| Public source repository | Approved by the owner on October 6 | Publish the reviewed clean-history source as vellora-windows; keep the existing development/audit repository private |
| Public Windows installer | Not approved | Use the new verified artifact, complete native/clean-account checks, confirm license/source companion and signing/known-limitations decision, then explicitly approve the release |
| Private security reporting | Enabled on the public source repository | Use the verified confidential route in SECURITY.md; no response-time guarantee |

Enabling private vulnerability reporting is a public-repository feature in
[GitHub's documentation](https://docs.github.com/en/code-security/how-tos/report-and-fix-vulnerabilities/configure-vulnerability-reporting/configure-for-a-repository).
The current private-state policy does not pretend that the future public route
has already been activated. No support response-time guarantee is promised.

If choosing to change an existing repository's visibility, remove or wait for
expiry of every unapproved private installer review artifact. Repository
visibility also changes access to existing Actions artifacts; a private upload
must not silently become an unintended public download. The prepared workflow
uploads installer/source review artifacts only when the repository is private,
so later public-source pushes cannot create a public installer channel. The
recommended fresh source repository avoids publishing the old private history,
PRs and artifacts in the first place.

Provider migration is also dated: the current optional cleanup snapshot is
scheduled to retire December 11, 2026, and the four offered transcription models
February 26, 2027. The [model lifecycle review](MODEL_LIFECYCLE.md) describes
replacement, response/language, cost and compatibility checks. No saved model
choice was changed and no replacement accuracy claim is made.

## Native checks still required for public installers

1. Install, upgrade and uninstall on a fresh Windows 10/11 account without
   development tools. Record WebView2 behavior, data retention and startup on/off.
2. Use dummy speech to test hold/lock/stop, long recording, device removal,
   permissions, sleep/resume, network/auth/rate failures and supported OpenAI
   models. Automated tests made no paid requests or real recordings.
3. Test Notepad/browser/elevated destinations, same-window focus changes, rich
   clipboard formats, concurrent clipboard updates and recovery after failure.
4. Check keyboard/screen-reader use, high contrast, two monitors and
   100/125/150/200 percent DPI in both appearances.
5. Confirm deletion and active-operation behavior: current dictation options
   are captured when recording stops; clearing history during processing can
   still be followed by that operation's result. Wait until idle or quit before
   applying retention/delivery changes and deleting data for a definite boundary.

These checks remain open; documentation of a limitation is not evidence that
the corresponding hardware/installer test passed. The remaining native gates
are tracked with acceptance criteria in the roadmap.

## Validation record: October 5 baseline

The hosted application candidate is
`7419e7eee8ecf39601e3feaa1e69bf9069921d05`.
[Its completed Windows validation run](https://github.com/Felix561/vellora/actions/runs/37279517885)
passed all three jobs on October 5, 2026.

| Check | Result and scope |
| --- | --- |
| Frontend | 39 behavioral regressions in six files; lint and both TypeScript projects passed |
| Rust | 44 regressions; formatting and Clippy with warnings denied passed |
| Dependencies and notices | npm/RustSec reported zero vulnerabilities; existing maintenance/platform warnings remain disclosed; 236 notice texts, zero unresolved package notices |
| Release bytes | Standalone EXE and actual extracted NSIS payload passed all five private-path categories; exact reviewed bundle-marker transformation and all three bundled notice hashes passed |
| Covered sources | Uploaded companion independently verified: artifact digest, 13 files, 12 checksums, five locked MPL archives, exact NSIS 3.11 source and all three committed notice copies |
| Repository privacy | Full-history secret scan passed; all six additional completed-run logs and the final 329,592-byte run log passed owner-path/key patterns and redacted Gitleaks |
| Final handoff | Documentation links, version synchronization, private-only uploads, production-fixture exclusions and formatting checked; the follow-up changes only documentation and four surplus EOF blank lines, with type checks repeated |

Two private review artifacts are attached to that run: the Windows installer
with checksums/`REVIEW_VERIFICATION.json`, and its covered-source companion.
They expire October 19, 2026. They are review materials, not public downloads or
evidence of a clean-account installation. The exact executable/installer hashes
are recorded in [publication privacy](PUBLICATION_PRIVACY.md).

The clean source handoff is a separate local one-commit repository and ZIP,
with no remote. Its full Git tree must equal the final reviewed source tree;
all 24 current image paths must exclude the 30 earlier image blobs from normal
and coding-tool checkpoint history. The handoff record includes source,
snapshot and archive hashes plus the redacted secret-scan result. This prepares
the proposed public content without exposing the private development history.

No local release executable or installer is built during this preparation.
The owner authorized one installed update after the 0.2.21 checks pass. That
existing-account update completed on October 6; details are below. Fresh-account
release checks remain open.
There is no automatic public
release or visibility change in the workflow. Pending dependency-update PRs
are separate proposed versions; do not merge them without their own notice,
test and artifact checks.

## October 6 shortcut follow-up

The reported Ctrl release/re-press while Win stays held previously looked like
a hands-free double tap. Locking now requires a complete release of both
modifier groups between taps. Resuming a hold cancels its own deferred stop;
session and release identities prevent an old timer from stopping another hold.

Review also found queued starts that could outlive a microphone failure or
automatic stop. Notifications now carry a reset epoch, checked when reserving
the session after credential lookup. Resetting invalidates earlier queued
gestures without clearing a newer gesture. Microphone and app operations run
outside the keyboard-state mutex. Help, setup and troubleshooting explain the
updated shortcut behavior.

The application candidate is `62b9297390e354e75e4147a7a7b86c8cf2b1b030`.
[Hosted run 37471723603](https://github.com/Felix561/vellora/actions/runs/37471723603)
passed all three jobs on October 6. The earlier 0.2.20 artifact evidence above
remains historical evidence only.

| Check | 0.2.21 result and scope |
| --- | --- |
| Behavioral checks | 62 Rust and 39 frontend tests; formatting, Clippy, ESLint, both TypeScript projects and production frontend build passed |
| Rendered GUI | Both appearances/all five pages and setup; selected actual Chromium keyboard/focus transitions, long text, forced colors, reduced-motion overlay and viewport/DPR reflow passed with synthetic IPC/clipboard data and no external network attempts |
| Dependency and source security | npm/RustSec audits found zero vulnerabilities; seven existing maintenance/platform warnings remain disclosed. Patched development-only source-map-js 1.2.2; 236 notice texts, zero unresolved package notices; full-history secret check passed |
| Release artifact | Standalone and actual installer payload each passed all five private-path categories; exact reviewed NSIS marker transformation and all three bundled notice hashes passed |
| Covered sources | Companion independently verified against this exact commit: ZIP digest, 13 flat files, 12 checksums, five locked MPL source archives, pinned NSIS source and three byte-exact notice copies; no traversal/symlink entries or sensitive metadata text findings |
| Repository log privacy | The final 335,744-byte combined log passed owner-path/key patterns and redacted Gitleaks with zero findings; preceding failed/cancelled runs were checked separately and produced no installer |
| Existing-account update | Owner-approved update from 0.2.18 to 0.2.21 executed once; installer exit 0, installed version/payload size/full hash and all three notices verified; startup registration preserved and app restarted |
| Owner shortcut test | After the update, the owner confirmed the originally reported Ctrl release/re-press while Win remains held now works; this is a reported physical reproduction check, not completion of all hardware/API tests |

The private Windows artifact is `11418377593`; the covered-source companion is
`11418267970`. Both expire October 20, 2026. The Windows artifact ZIP digest,
installer/payload checksums and companion verification are in
[publication privacy](PUBLICATION_PRIVACY.md). No public release/tag or visibility
change was made.

The update used the reviewed current-user NSIS `/S /UPDATE` path, checking both
registered install destinations and executable identity before launching it.
The installed payload is exactly the reviewed NSIS-marked executable. No personal
settings/history/credential contents were inspected for verification. Running
on the owner's existing account does not establish fresh-account install,
uninstall, broader microphone/shortcut/clipboard behavior, screen-reader support
or native DPI/multi-monitor compatibility. The owner separately confirmed the
original shortcut reproduction is fixed after this update. The remaining native
gates above stay open.

Documentation follow-ups receive link, version, privacy and whitespace checks.
The refreshed clean-history handoff preserves the final source tree in one
commit with no remote and excludes all 30 earlier image blobs. Public source
and public installer approval remain separate decisions. Source publication
is now approved; an installer release remains deferred.
