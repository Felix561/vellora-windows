# Source publication status

Vellora is an early Windows source beta, published under MIT at
[Felix561/vellora-windows](https://github.com/Felix561/vellora-windows) with the
owner's approval on October 6, 2026. Felix Seitzer remains the credited author.

The public repository starts from a reviewed clean-history snapshot. The
original development repository, its previous artwork, private review
installers and coding-tool checkpoints remain in the private audit archive.
Current artwork and screenshots have [recorded provenance](ARTWORK_PROVENANCE.md)
and the screenshots use synthetic data.

## Source now, installer later

There is no public EXE or installer release. Follow the root
[development instructions](../README.md#development) to run from source.
Public CI checks the source, production frontend assets, rendered GUI,
dependencies and secrets. It does not compile or upload release executables
or installers and makes no paid OpenAI requests.

Fresh-account install/uninstall, broader hardware and clipboard scenarios,
screen readers and native DPI/multi-monitor checks remain on the
[roadmap](ROADMAP.md). An existing-account update and the owner's reported
shortcut reproduction check passed during private preparation; they do not
close those broader release gates. Installer distribution requires a later
release decision, verified checksums/notices and required covered sources.

## Review evidence and support

The dated audit, privacy, licensing and publication-review documents preserve
the preparation record. Their historical Actions links refer to the private
development archive and are not publicly accessible build evidence. Current
public checks are visible in
[GitHub Actions](https://github.com/Felix561/vellora-windows/actions/workflows/ci.yml).

Use the public issue forms for bugs and focused Windows feature requests.
Report vulnerabilities through the confidential route in
[SECURITY.md](../SECURITY.md). Never share real API keys, transcripts, recordings,
clipboard contents, personal directories or unredacted diagnostics.
