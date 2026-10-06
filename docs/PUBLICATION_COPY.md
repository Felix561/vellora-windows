# Source-publication copy

This document originally prepared README wording for a later source-publication decision. The maintainer has now authorized a fresh public source repository at [Felix561/vellora-windows](https://github.com/Felix561/vellora-windows). That source-only wording is applied in the main [README](../README.md). Public installers and executable releases remain deferred.

## Applied source-only publication changes

The public source is based on the reviewed independent clean-history candidate. The existing development/audit repository remains private; its old history and installer artifacts are not made public by this source publication.

The README now describes an **early Windows source beta** under MIT, uses public repository/CI links, and directs readers to [run from source](../README.md#development). It retains the synthetic screenshot gallery and provenance, cloud-transcription/API-billing disclosures, optional cleanup, local-history limitations, model lifecycle work, and Windows-only scope. It does not direct readers to private Actions artifacts or imply that an approved public download exists.

Public source CI validates code and synthetic behavior without building/uploading release installers or publishing executable downloads. Source publication does not approve binary distribution. Historical private review evidence remains labeled in the [publication review](PUBLICATION_REVIEW.md).

## Repository presentation

The About description remains:

> Windows dictation with OpenAI, local text history, a click-through indicator, and Notebook / Windows Classic themes.

The homepage points to the public repository README. Windows-only topics, the MIT license, and reviewed asset provenance match this source release. [SECURITY.md](../SECURITY.md) documents the private vulnerability-reporting route; vulnerabilities should not be posted in normal public issues.

## If an executable release is approved later

Replace the README's **Install** section with the tested release's download link, exact version and platform requirements, installation steps, SHA-256 checksum, signing status, known limitations, and upgrade/uninstall data-retention behavior. Link the actual GitHub release only after it exists. Keep Windows and antivirus protections enabled.

Before enabling binary publication, complete the applicable [release checks](ROADMAP.md) and redistribution/source-availability requirements and obtain a separate maintainer decision. The source-only workflow must not acquire installer uploads implicitly.

Keep the synthetic screenshots, cloud-transcription and optional-cleanup disclosures, local-history limitations, and API-billing notice. A short dummy-data dictation video and a release-status badge can be added later; neither is needed for source publication.
