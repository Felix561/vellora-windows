# Contributing

Vellora is a public Windows source beta built with React, TypeScript, Vite, Rust, and Tauri 2 under the MIT license. Windows is the only supported platform; changes should stay focused on its dictation workflow. Public executable releases are deferred.

## Development setup

Use Windows 10/11, Node.js 22.22.2 or later, Rust 1.95.0, the Microsoft C++ Build Tools, and WebView2. Follow [Tauri's Windows prerequisites](https://v2.tauri.app/start/prerequisites/#windows).

```powershell
npm ci
npm run tauri:dev
```

Use the committed npm and Cargo lockfiles. Configure a personal OpenAI API key through Vellora's GUI only when manually testing dictation. Builds and automated tests do not need a key.

If a managed network uses a trusted HTTPS proxy, configure its CA locally. Do not commit certificates, personal proxy settings, `.npmrc`, API keys, local settings, transcripts, audio, database files, or installers. Do not bypass TLS verification.

## Validation

```powershell
npm run check:frontend
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked
npm audit --audit-level=low
cargo audit --file src-tauri/Cargo.lock
```

`check:frontend` runs ESLint, app/test type checks, and the Vitest regression suite. Use `npm test` to run the tests once or `npm run test:watch` while developing. Frontend regression tests use synthetic data and do not contact OpenAI or read the installed app's credentials, history, or clipboard. They complement the browser smoke procedure in [GUI_TESTING.md](docs/GUI_TESTING.md); neither replaces native microphone, shortcut, paste, or installer testing.

Install the audit command with `cargo install cargo-audit --version 0.22.2 --locked` if needed. Refresh dependency attributions with `node .github/scripts/generate-notices.mjs` after lockfile changes. The generated missing-notices list must be resolved before a public binary release. Build an executable only when needed to validate native integration or installation, rather than after each interface/docs change:

```powershell
. ./.github/scripts/configure-release-paths.ps1
npm run tauri:build -- --ci -- --locked
node .github/scripts/check-release-privacy.mjs --file src-tauri/target/release/vellora.exe
```

For optional local installer testing, the installer is written to `src-tauri/target/release/bundle/nsis/`. Compiler remapping reduces embedded build-root disclosure; it does not replace the binary scan. On a machine with 7-Zip, `verify-review-installer.ps1` scans the extracted payload and verifies its hash against the exactly expected Tauri NSIS-marker patch; every other byte change is rejected. It also checks all three notice hashes without executing the installer. These instructions do not authorize distribution of a locally built installer. See [PUBLICATION_REVIEW.md](docs/PUBLICATION_REVIEW.md) for release gates and historical private-build evidence.

The [public source CI workflow](https://github.com/Felix561/vellora-windows/actions/workflows/ci.yml) runs frontend and Rust validation, synthetic rendered-GUI tests, dependency audits, and secret scanning. It does not build/upload release installers, publish executable downloads, or attach private review artifacts. Future binary distribution needs its own approved release workflow and covered-source/notice plan.

Check affected screens in Notebook and Windows Classic themes. Test keyboard focus, resizing, long text, loading and error states, and the click-through status indicator. For recording or paste changes, verify failed requests leave recoverable text, microphone failures are visible, and an unrelated clipboard update is preserved.

## Pull requests

Keep changes focused. Explain the triggering problem, resulting behavior, and validation. Add regression tests for meaningful behavior changes. Treat transcripts and clipboard text as untrusted data; render them as text and avoid injecting HTML or shell fragments. Route privileged operations through narrowly scoped Rust commands and keep the frontend capability set minimal.

For bugs, use the [Windows bug-report form](https://github.com/Felix561/vellora-windows/issues/new/choose) and include a small reproduction with dummy data. **Help & About > Copy support information** provides an allowlisted support summary; review it before sharing. Do not attach full logs, settings/history databases, keys, recordings, or private screenshots. Use [SECURITY.md](SECURITY.md) for vulnerability reports.

## Future executable release checklist

- Update `package.json`, `package-lock.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, and `src-tauri/tauri.conf.json` to the same version.
- Add release notes to [CHANGELOG.md](CHANGELOG.md), update user-facing help/troubleshooting if behavior changed, and record completed/open work in the roadmap.
- Confirm all CI jobs and security scans pass for the release commit.
- Review dependency licenses and refresh bundled third-party notices.
- Include native component notices and a durable covered-source download/instructions where required; expiring private Actions artifacts are not the public source-availability plan.
- Install, upgrade, and uninstall on a clean Windows account; check data migration and startup enabled/disabled behavior.
- Verify both themes, recording, microphone/device failure, network failure, cleanup disabled/enabled, transcript deletion, and paste recovery.
- Confirm no personal paths, recordings, screenshots with private text, keys, certificates, or local history enter the source archive or release artifacts.
- Remap compiler roots, pass the unpacked binary privacy check, and verify the installer's payload/notice hashes before uploading artifacts.
- Sign public installers where possible and document unsigned installer warnings when signing is unavailable.
- Provide checksums, supported Windows versions, model/network requirements, release notes, and known limitations.
- Confirm the privacy documentation matches the shipped version.
- Review model lifecycle/deadlines and test any replacements before changing saved model choices or cost estimates.
- Keep public source CI separate from binary publication; do not enable installer uploads or releases without a separate maintainer decision.
