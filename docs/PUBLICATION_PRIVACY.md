# Publication privacy review

> **Source publication update, October 6, 2026:** the owner approved a fresh
> public Windows source beta. This document retains the dated private
> preparation record; historical CI links refer to the private audit archive.
> See [current publication status](PUBLICATION_STATUS.md). Public installer
> distribution remains deferred.

Reviewed October 4 through 6, 2026; current candidate: Vellora 0.2.21, reviewed October 6.
The October 4 baseline and October 5 0.2.20 evidence remain dated below and do
not identify the current release bytes. This report concerns material that
could become visible when the repository or an installer is published. Runtime
audio, history and clipboard behavior is described separately in [PRIVACY.md](PRIVACY.md).

**Source result:** no accidental personal directory, personal email, real API
key, recording or transcript was identified in the reviewed source/history.
**Artifact result:** the old locally built 0.2.19 executable contains absolute
developer build paths and is not cleared for public distribution. The current
hosted 0.2.21 candidate passed direct standalone/payload path checks, exact
installer-resource verification and independent covered-source verification.
One authorized existing-account update completed with matching installed bytes
and notices. Fresh-account installation and the remaining native runtime checks
remain open; the owner's shortcut report is recorded separately below.

This is a documented review with explicit limits, not a guarantee that every
possible kind of personal information has been detected. The repository remains
private pending the owner's final review and approval.

## October 4 and 5 source/history baseline

The baseline was commit `fa0eda5ffc6e7e47df8f60dd7f3aebfec78657cc`.
The initial 0.2.20 hardening candidate was commit
`5682afeb6a74de2d30b7945d11f5eaeb5ed7b1a5`. Its staged scan inspected approximately
180.82 KB without findings; the subsequent redacted full-history Gitleaks scan
covered 44 reachable commits and approximately 10.22 MB of diffs without findings.
Documentation-only follow-ups receive a further staged scan before their push.

The subsequent UNC-scanner correction passed a redacted scan of 45 reachable
commits (approximately 10.23 MB). The first hosted build's 309,688-byte text log
also passed owner-path/provider-key/private-key-header patterns and Gitleaks.
That build blocked uploads on a reproduced invalid-UNC concatenation, as
explained below; its installer was not cleared by inference.

After the owner requested newly documented artwork, all three branding PNGs,
five Windows icon files and sixteen screenshot files were replaced. The new
PNG metadata inspections found no EXIF, XMP, comments or personal fields; the
ICO contains its size inventory. All sixteen captures were visually reviewed
and all prior screenshot hashes changed. The new generation used text alone;
the paper edit used only the new master. Exact asset hashes/prompts are recorded
in [artwork provenance](ARTWORK_PROVENANCE.md). Older unverified artwork stays
in the private history boundary rather than the clean publication snapshot.

| Reviewed material | Scope and method | Result |
| --- | --- | --- |
| Tracked checkout | 104 files at the baseline; file names and text patterns | No accidentally committed data/credential files identified |
| Reachable Git content | 43 commits across all 39 local refs; 313 distinct blobs totaling 11,768,509 bytes | No accidental personal path or email identified after classifying matches |
| Commit identities and messages | Author/committer metadata and messages for all 43 commits | GitHub noreply emails throughout; no sensitive-pattern message matches |
| Secret scan | Gitleaks 8.30.1, `git --log-opts="--all" --redact=100`; approximately 10.04 MB of history diffs | No findings; scanner exit status 0 |
| Baseline image content | At the baseline: all 16 screenshots, 7 brand/icon PNGs and 5 earlier PNG variants visually inspected | Synthetic examples or artwork; no personal transcript, key, account/device name or local directory shown |
| Baseline image metadata | At the baseline: all 28 reachable PNG blobs and 2 ICO blobs decoded and inspected | No EXIF, XMP or textual metadata; older icons contain ordinary DPI/gamma/sRGB fields only |
| Existing local release EXE | Static ASCII and UTF-16 inspection; no execution or rebuild | Failed: embedded Windows user-profile and source/debug build paths |
| Compressed NSIS installer | Existing file identified by SHA-256; plain-string check only | Not cleared: compression can conceal the executable's strings |

Of the 43 reachable commits, 17 belong to normal `main` history. The additional
26 are local coding-tool checkpoint commits and are not ancestors of `main`.
All local branches, remote-tracking refs, tags and checkpoint refs were included
in the history review. Publication should use only intended branches/tags;
never use a mirror push to publish local tool checkpoint refs.

Gitleaks' Windows archive SHA-256 matched its stored release checksum:
`d29144deff3a68aa93ced33dddf84b7fdc26070add4aa0f4513094c8332afc4e`.
Its report and the detailed pattern inventory remain in ignored local review
storage. They are not application diagnostics and should not be uploaded.

## Matches reviewed rather than silently ignored

- The three absolute Windows path occurrences in source are deliberately
  invented privacy-regression fixtures in `src-tauri/src/support.rs` and
  `tests/frontend/help.test.tsx`. Their purpose is to prove that support output
  omits free-form paths and other sensitive fields.
- Token-shaped strings in onboarding/settings tests are one plainly fake test
  key repeated in assertions. No paid request is made by those tests.
- Email-shaped matches outside Git metadata are icon filenames containing
  `@2x` and author/contact notices copied from third-party license texts. Those
  license notices are intentional and must not be removed as a privacy fix.
- `%APPDATA%`, `%LOCALAPPDATA%` and `%TEMP%` paths document Windows storage
  locations without revealing a particular user's directory. Loopback URLs and
  fixed test data describe local development or isolated test behavior.
- The project author credit and GitHub repository handle are deliberate public
  project identity. The owner approved retaining the author name. They appear
  in the license, Help view/screenshots and repository links.
- Screenshot saved-key status, dates, charts, spend estimates and the example
  history sentence are simulated; they are not readings of the installed app.
  Their provenance is recorded in [images/README.md](images/README.md).

No reachable tree contained a tracked environment file, credential file,
database, audio recording, executable/installer, log or memory dump under the
reviewed sensitive-file patterns. Existing ignore rules cover these common
local artifacts; ignore rules do not erase material already in Git history.

## Historical local 0.2.19 installer: do not publish

The reviewed executable is `src-tauri/target/release/vellora.exe`, 15,744,000 bytes,
SHA-256:

```text
b0105df3656fe6b0876791aa87b03361d97074de880d380471a185ef33936088
```

The release privacy scanner found 1,020 Windows user-profile path occurrences,
758 source/debug path occurrences, and zero network source/debug paths.
Categories overlap; these are occurrence counts rather than a count
of distinct directories. The finding does not indicate a stored API key or
dictation history. Compiler/dependency source-location strings alone can reveal
the machine owner's directory layout, so this artifact still fails the requested
privacy standard.

An initial network-shaped candidate was checked locally and proved to be the
ordinary Windows `\\.\NUL` device literal concatenated with a neutral `/rustc/`
standard-library source-location string. The scanner now distinguishes that
literal from normal and extended UNC paths. Targeted self-tests cover both
extended UNC forms and this benign concatenation; no broad finding suppression
was added.

The first hosted 0.2.20 build also found one network-shaped candidate after
remapping, with zero user-profile, Windows source/debug or explicit build-root
matches. It blocked all artifact uploads. Replacing the old EXE's known prefixes
in memory reproduced one invalid UNC candidate: doubled backslashes in a public
character lookup table joined a neutral `/cargo` source string. The scanner now
excludes `/` from the UNC server component, with a specific concatenation
regression and retained standard/extended UNC positives. This follows
[Microsoft's UNC component format](https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file).
The in-memory reproduction does not certify new compiled bytes; the corrected
check must pass on the actual hosted build before it becomes a review artifact.

The associated installer is `Vellora_0.2.19_x64-setup.exe`, 4,636,568 bytes,
SHA-256:

```text
fdd2d1207d4a4a4f1e5944004259f98663474099b71a21ab6fc2a7e3333283b9
```

Checking the compressed installer for plain text found no profile strings, but
that cannot clear its compressed executable. Neither existing local artifact
should be used as the future public download.

For the release candidate, remap compiler paths for the workspace, Cargo home
and Rust toolchain/sysroot to neutral build roots. Prefer a locked build on a
GitHub-hosted Windows runner, then inspect the actual release EXE before uploading
it as a review artifact. Tauri CLI 2.11.2 marks the packaged application as an NSIS
bundle and restores the standalone executable afterwards. Validate the payload
against the exactly expected marker patch, reject every other byte change, and
scan the extracted payload directly. An expected payload hash match is better
evidence than scanning the compressed container.
Rebuilding is not a substitute for scanning the resulting bytes.

The exact marker transformation is pinned to the
[reviewed Tauri CLI implementation](https://github.com/tauri-apps/tauri/blob/499df79be65ef8c0670abc0207cd9e37b55d8491/crates/tauri-bundler/src/bundle.rs).
Both locked CLI packages must remain at 2.11.2. Verification requires one
original marker and full-file size/SHA-256 equality after its `UNK` to `NSS`
replacement; unrelated changes, missing/duplicate markers and version changes
fail the gate. This is followed by a separate direct payload privacy scan.

## Repeatable release artifact check

The repository contains a small count-only scanner:

```powershell
node .github/scripts/check-release-privacy.mjs --self-test
node .github/scripts/check-release-privacy.mjs --file src-tauri/target/release/vellora.exe
```

Build automation should also supply each actual build root with repeated
`--forbid-path` arguments. It detects matching Windows/Unix user-profile paths,
Windows/UNC source and debug-file paths, and explicit roots in ASCII and UTF-16
at both byte alignments. It emits SHA-256 and category counts, never matched path
values. Exit status is 0 for no matches, 1 for findings, and 2 for invalid input.
The self-test covers synthetic user paths, Unicode encodings, workspace paths,
URL false positives, device-literal concatenation and safe remapped roots. It
does not compile anything.

Scan the unpacked application EXE. An installer is also a Windows executable,
but this script cannot decompress its payload and cannot certify that payload
when pointed only at the installer container.

## GitHub-visible surface review: October 4 baseline

Repository-scoped GitHub APIs were read on October 4, 2026. No visibility,
settings, subscriptions, comments, releases or Actions runs were changed.
API payloads and text-log archives remain in ignored local review storage;
application or installer assets were not downloaded or executed.

| Surface | Reviewed scope | Result |
| --- | --- | --- |
| Repository metadata | Description, homepage, topics, visibility and feature flags | Private; intentional repository identity; wiki and discussions disabled |
| Issue/PR text | All 16 PRs, including 14 closed and 2 open; all titles/bodies and metadata returned by the paginated API | All PRs are Dependabot updates; no accidental personal directory/email or credential identified |
| Comments/reviews | 15 issue/PR conversation comments; zero inline review comments, reviews or commit comments | No personal-data or secret finding identified |
| PR file diffs | Full authenticated diffs for all 16 Dependabot PRs, checked against paginated changed-file metadata | All available; 23 changed-file records across 4 unique files; no sensitive-pattern or Gitleaks findings |
| Releases/downloads | No releases or release assets listed | Nothing available to inspect in these surfaces |
| Actions metadata | All 42 listed runs, including 17 push, 17 PR and 8 Dependabot runs; 110 job records across attempts | No accidental personal data identified |
| Actions text logs | All 42 listed run archives available; 176 text log entries totaling 13,576,351 bytes | Standard hosted-runner/Dependabot paths and public bot metadata; no owner directory/email or credential identified |
| Actions artifacts | Metadata for 2 unexpired Windows-installer artifacts | Names, sizes, expiration and run associations inspected; binary contents remain unverified by this surface review |
| Linked attachments | Issue/PR/comment text checked for GitHub upload/user-attachment links | No such attachment links identified; external linked content was not downloaded |

The Actions text review includes the main-app and dependency-PR histories,
including failed runs. All 42 archives listed at this snapshot were readable;
none was expired or unavailable. Their two installer artifact names identify
commits `a7d0b58` and `3b5a428`; neither artifact was treated as a newly verified
public release candidate.

The PR diff review examined 202,359 bytes of patch content across
`.github/workflows/ci.yml`, `package.json`, `package-lock.json` and
`src-tauri/Cargo.lock`. Every full diff's file-header count matched its
changed-file metadata, and no binary-diff marker was present. Personal-path,
email, provider-token and private-key-header patterns returned no matches.
Gitleaks scanned the unmodified diff bytes directly through standard input,
with fully redacted reporting, and returned no findings (exit status 0).
Only ignored, sanitized patch/metadata snapshots were retained locally.

Pattern matches were individually classified. Emails belonged to public
Dependabot/GitHub automation, GitHub noreply identities, package-version strings
and SSH remote syntax. Directory matches were GitHub-hosted runner paths or the
Dependabot service account, rather than the owner's Windows user directory.
Those generic build/service paths do not identify the owner's machine.

The initial authenticated repository API response also included a GitHub
temporary clone-token field. It was generated by the API response, not found
in a committed file, PR/comment or Actions log. The value was never printed or
uploaded and the field was removed from the ignored local audit payload. This
is distinguished from a pre-existing repository leak. A second redacted
Gitleaks scan of the sanitized API text and log archives, with archive/encoded
content traversal enabled, examined approximately 15.90 MB and reported no
findings (exit status 0).

The public surface review must be repeated for new PR/comment/log content added
after this snapshot. Deleted or unavailable historical GitHub records cannot
be certified by the current listing. External link contents and artifact
payloads were not part of this GitHub text review.

### October 5 read-only follow-up

The subsequent metadata/text review covered 63 read-only endpoints and 16,688
string values. The repository was still private, with 16 PRs (14 closed and
two open), no ordinary issues, 15 conversation comments, no reviews/inline
comments/commit comments, and no releases. No owner-directory, provider-key,
private-key-header or uploaded-attachment candidate was identified. All 137
email-shaped matches were classified as GitHub remote/service syntax or
package-version references; approved author credits remain intentional.

Seven newer Actions runs were listed beyond the 42-run baseline. The six
already completed runs supplied 28 readable text-log members totaling
1,080,083 bytes. Owner-profile/workspace/provider-key/private-key-header
patterns and fully redacted Gitleaks checks returned no findings. Their actual
outcomes were three failures, one cancellation and two successes; absence of
sensitive text does not turn a failed/cancelled build into release validation.
The final candidate run is recorded separately below. Installer payloads were
not downloaded by this repository-surface review.

### October 6 read-only surface follow-up

At 13:21 UTC, the review added 22 metadata/detail endpoints and 7,670 string
values, including two updated Dependabot PRs and their 28,313 bytes of diffs
across four file sections. All requested surfaces were available; no binary
diff sections were present. Classified matches were public bot/SSH repository
syntax, package-version references and generic hosted-runner paths, with no
unclassified owner-directory, personal-email or credential finding. The
authentication-only clone-token field was discarded before storing evidence.

A separate 13:26 UTC redacted Gitleaks check covered 18 metadata/PR-diff
endpoints and 1,298,801 bytes, with zero findings and no unavailable surfaces.
The four newer run records included three completed runs and one still in
progress at the snapshot. All three completed logs were available, totaling
355,269 bytes; their path/key pattern and redacted Gitleaks checks passed.
The subsequently completed current run has its own final log/artifact evidence
below. This follow-up downloaded no artifact payload and changed no GitHub
surface. Later records still require their own checks.

## Historical hosted candidate: October 5, 0.2.20

The hashes and artifact records in this section belong only to 0.2.20. The
current 0.2.21 candidate is recorded in the following section.

[Run 37279517885](https://github.com/Felix561/vellora/actions/runs/37279517885)
passed all Windows, dependency and secret jobs for source commit
`7419e7eee8ecf39601e3feaa1e69bf9069921d05`.
The actual standalone and extracted payload are both 16,923,136 bytes. Each
returned zero in all five path-finding categories. Their hashes differ only
because the payload is the exact reviewed NSIS-marked variant; the full-file
comparison passed, with one marker at byte offset 12,940,792.

| Verified material | SHA-256 |
| --- | --- |
| Standalone release executable | `479f18993214e00cd3cff2f0f24c96aaec7be389e24ff24c044f8696a53e90a2` |
| Expected and actual extracted NSIS payload | `82185ce6352baf1d9b3a18d8a1eb26003748586c71dfe3cb6279d9d4b7b78e77` |
| `Vellora_0.2.20_x64-setup.exe` | `dca144bfab94fed42ea2f5cc59ef9ebbcb4e050eb1136c2b30d9a7691f765dd0` |

The extracted `LICENSE`, `THIRD_PARTY_NOTICES.md` and
`THIRD_PARTY_NOTICES_NATIVE.md` each matched their committed source hash.
Verification used 7-Zip extraction on the hosted runner and did not execute
the installer or its payload. Schema 2 of `REVIEW_VERIFICATION.json` preserves
the hashes, expected marker patch and direct payload privacy counts.

The final 329,592-byte combined run log passed owner-profile/workspace,
provider-key/private-key-header patterns and redacted Gitleaks, with zero
findings. Its two new private artifact metadata records bind to that source
commit and expire October 19. This adds two verified review artifacts to the
earlier two unverified artifact records; it does not clear the earlier binaries.
At that review stage, no application executable or installer was downloaded to
the owner's PC. The separately authorized October 6 update is described below.

The uploaded covered-source ZIP was separately inspected in memory: its
4,567,716 bytes matched artifact SHA-256
`fb314cd7ae342ed37e56f482431c75be17c79eeb7b997aff2d3ff507aefb6596`.
It contains the 13 expected flat files, no traversal/symlink entries and 12
complete checksum records. Its version/source-commit manifest, five Cargo.lock
archive pins, exact NSIS 3.11 source pin, recovered MPL notice and all three
notice copies matched the committed sources. No sensitive plaintext metadata
was found. Nested source archives were not extracted or executed. License
copies were compared to Git blobs; the local LICENSE differs only in Windows
line endings, not content.

The 0.2.20 clean-history source export was checked for full tree equality,
exactly one commit, no remote, a redacted secret scan and no earlier artwork
among the 24 current image paths. The historical exclusion covered all 30
earlier PNG/ICO blobs across normal and coding-tool checkpoint history. That
earlier snapshot is historical; it does not include the October 6 changes.

## Current verified candidate: October 6, 0.2.21

Source commit `62b9297390e354e75e4147a7a7b86c8cf2b1b030` passed all three
jobs in [run 37471723603](https://github.com/Felix561/vellora/actions/runs/37471723603),
including the rendered GUI, behavioral checks, dependency checks and full-history
secret check. The standalone executable and actual extracted NSIS payload are
both 16,950,272 bytes and each returned zero in all five private-path categories.
The payload matched the full-file, exactly reviewed Tauri CLI 2.11.2 marker
transformation, with one marker at byte offset 12,962,840. All three extracted
license/notice files matched their source hashes.

| Current 0.2.21 material | SHA-256 |
| --- | --- |
| Standalone release executable | `895d842c1795f8ae3aa3a9d0c7c29ba60e45dee535a87a420e4b90a838a821e2` |
| Expected and actual extracted NSIS payload | `00626301839febc1e0ddcc062034ccba0089049ed2306d14ebda7801be57bd3f` |
| `Vellora_0.2.21_x64-setup.exe` | `08e05eeeb083efa5edc3f3c831e3bc71a8b343001645b9d738a42ce0c8a5219f` |
| Private Windows artifact ZIP | `93ad54f10b9206a0248354dd6e6a838be383ff02c28bb8fdbf0e85f2966cbf85` |
| Private covered-source artifact ZIP | `bc45c059ee1fc5c7d1ad9cc2854cdea7903b02f30bb02895cda8cc1966401407` |

The Windows artifact is `11418377593`; its installer is 5,890,643 bytes. The
covered-source artifact is `11418267970`. Both bind to the source commit above
and expire October 20, 2026. They remain private review materials. Schema 2 of
`REVIEW_VERIFICATION.json` binds original, expected patched and actual payload
hashes, the exact marker change, direct payload privacy counts and all three
notice hashes. Hosted archive verification did not execute either executable.

Independent verification downloaded only the covered-source ZIP into memory.
Its 4,567,716 bytes matched the authenticated artifact digest and the separately
supplied digest above. It contained exactly 13 flat files, 12 complete checksum
records and no traversal/symlink entries. Five MPL crate archives matched the
exact candidate's Cargo.lock; the NSIS 3.11 source archive matched its pinned
commit and checksum. The version/commit manifest, recovered MPL notice record
and all three notice copies matched the committed sources. Sensitive plaintext
metadata findings were zero. Nested source archives were not extracted or
executed, and that independent source review did not download a Windows
artifact. Comparing notices against Git blobs avoids mistaking the local
LICENSE's Windows line endings for a content change.

The final 335,744-byte combined run log passed owner-profile/workspace,
provider-key/private-key-header patterns and fully redacted Gitleaks with zero
findings. This log result concerns that run's text; it is separate from the
direct executable/payload checks. Earlier failed or cancelled candidates do
not inherit the successful run's artifact validation.

### Authorized existing-account update and owner report

The owner authorized one installed update from 0.2.18 to 0.2.21. That update
executed once on October 6 and returned installer exit status 0. The installed
application version, 16,950,272-byte executable and full payload hash matched
the current reviewed NSIS payload. All three installed notice hashes matched;
enabled startup registration was preserved and the application restarted.
No personal settings/history/credential contents were read for verification.
The safe evidence records remain in ignored local review storage.

After the update, the owner reports that the original physical shortcut
sequence works. This records the owner's observation for that sequence.
The automated installed-update check did not perform a physical shortcut or
microphone test. Neither that check nor the owner report establishes every
native release gate: fresh-account installation/uninstallation, speech/device
failures, clipboard destinations, screen readers and native DPI/multi-monitor
behavior remain separate checks.

### Source/history follow-up and final handoff requirements

An earlier October 6 source/history follow-up bound to commit
`f0e9a94ee21b3d372412c19eaef5cdf4a17ea9ac` examined 50 commits and 382 text
blobs totaling 17,935,732 bytes. Its 51 binary blobs were unchanged from the
earlier review. The twelve pattern matches were classified as synthetic
scanner/help/key fixtures; there were zero unclassified matches, exact
owner-path matches or Gitleaks findings. That report predates the current
application commit; it does not claim a fresh manual inspection of every
later blob. The current application candidate separately passed CI's
full-history secret check.

The final local source follow-up at application HEAD
`62b9297390e354e75e4147a7a7b86c8cf2b1b030` covered 53 reachable commits across
39 refs, 386 text blobs, 51 binary blobs and 120 current tracked files. Redacted
Gitleaks found zero leaks in history, tracked source and the six-document
working diff inspected at that scan. All reviewed author/committer emails were
GitHub noreply identities; sensitive commit-message matches were zero. Broad
string matches were classified as synthetic fixtures, upstream license
contacts and npm/icon filenames. Documentation additions after that scan and
the final committed history still require repeat checks before the private
handoff push.

The clean-history source snapshot must be regenerated after the final
documentation commit and checked against that final reviewed Git tree. Require
exact full-tree equality, exactly one commit, no remote, a redacted secret scan
and no earlier artwork among the 24 current image paths. The historical image
exclusion must include all 30 earlier PNG/ICO blobs across normal and local
coding-tool checkpoint history. Record the final source/snapshot/archive hashes
in the separate ignored handoff evidence, rather than reusing an older snapshot
hash in this report. Documentation follow-ups require staged privacy, link,
version and whitespace checks before their private push.

The current installer remains bound to its application source commit above;
later documentation-only handoff changes do not create different installed
bytes or certify a future application change. No public repository, release,
tag or visibility change has been made.

## Final approval checklist

- [x] Pass the full-history secret check for the current 0.2.21 application
      candidate. The earlier source/image/manual history review remains dated
      above; subsequent staged text and final committed follow-ups require
      their corresponding repeat checks.
- [x] Regenerate and verify the final independent one-commit/no-remote source
      handoff after the documentation commit, using the requirements above.
- [x] Review currently available repository-side text/metadata surfaces: issues,
      PR descriptions/comments, Actions logs/artifact metadata, release listings
      and linked attachment references (October 4 snapshot above).
- [x] Inspect the October 5 repository-side delta and its six completed-run
      logs plus the 0.2.20 final log; inspect the October 6 surface/diff follow-up
      and three completed-run logs, then separately check the current final
      log and verify its installer payload. New/unavailable GitHub content
      is subject to the limits below.
- [x] Use a freshly verified release EXE with path-remapping checks passed; do
      not reuse the flagged local 0.2.19 installer.
- [x] Match the payload to the exactly expected NSIS-marked executable, scan
      that extracted payload and record its checksum and all bundled notices.
- [x] Independently verify the current covered-source artifact and execute
      the one authorized existing-account update with matching bytes/notices.
- [ ] Complete fresh-account Windows installation/uninstallation and the
      remaining native runtime checks separately.
- [ ] Confirm author/branding rights and final public credits separately from
      this privacy review.
- [ ] Obtain the owner's explicit final approval before visibility changes or
      publishing downloads.

## Limits

Pattern scanning cannot recognize every personal sentence, obfuscated secret,
arbitrary binary encoding or steganographic content. Visual review is manual,
not exhaustive OCR. The release path scanner does not unpack archives and is
not a general secret or malware scanner. No installed API key, personal SQLite
history, clipboard contents, microphone recording or account credentials were
read for this publication review. No personal material was uploaded.

The October 4 manual reachable-blob/image/identity review and the dated GitHub
surface inventories remain bounded to their documented snapshots. Later automated
full-history/staged scans and the October 6 log/artifact checks add evidence
within their stated scope; they do not repeat every manual historical-content
inspection or certify every new GitHub issue, comment, attachment or log.
Unavailable/deleted historical records and external linked content remain
unverified. The fresh source handoff must receive its own final checks.

This review does not cover future commits, unreachable objects/reflogs, other
repositories/backups, unsigned installer trust or native behavior beyond the
specific installed-update evidence and owner report above. These limits retain
the final review gate; they do not indicate a leak in the reviewed material.
