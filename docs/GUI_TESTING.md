# GUI smoke testing

> **Source publication update, October 6, 2026:** the owner approved a fresh
> public Windows source beta. This document retains the dated private
> preparation record; historical CI links refer to the private audit archive.
> See [current publication status](PUBLICATION_STATUS.md). Public installer
> distribution remains deferred.

The frontend smoke test runs the real React interface against a mocked Tauri
backend. It covers error feedback, saved-model state, transcript recovery,
delayed-paste cancellation, history privacy controls, keyboard focus, and both
appearances on every page at 1040, 880, and 520 pixels wide. It also exercises
Help & About, safe support information, and all four guided-setup steps.

All transcripts, settings, costs, and API-key status in this test are synthetic.
It makes no OpenAI requests and does not read or change the installed Vellora
application's credentials, history, settings, or clipboard. A successful result
does not verify native Windows hotkeys, microphone capture, clipboard behavior,
the click-through overlay, or installer operation; those require separate native
testing.

## Prerequisites

- Windows with Node.js 22.22.2 or later and npm.
- Chrome or Edge installed.
- A checkout of the repository with dependencies installed using `npm ci`.
- Local ports 1420 (Vite) and 19329 (the isolated test browser) available.

Rust and an OpenAI API key are not needed for this frontend test.

## Automated regression checks

`npm run check:frontend` runs ESLint, app/test TypeScript checks and Vitest
regression tests in jsdom: 39 tests in six files for Vellora 0.2.21. These checks
run in CI without opening a browser or building an executable. They cover
persistence failures, startup configuration,
theme saving, history deletion confirmation, recovery without history,
delayed-paste cancellation, setup reminders and honest practice feedback,
clipboard fallback and diagnostic privacy. They also cover out-of-order
refreshes, stale errors, settings/status events arriving during reads,
serialized settings edits, history deletion and changing the displayed
transcript during a paste countdown. Run `npm test` for tests alone.

These tests exercise React behavior against synthetic backend responses. They
do not measure rendered layout, native microphone capture, Windows paste,
overlay hit testing, or OpenAI model accuracy. Use the optional browser smoke
test below for layout and keyboard review, and the publication roadmap for
native release checks.

### Hosted rendered checks

Windows CI runs [scripts/gui-smoke.cjs](../scripts/gui-smoke.cjs) through
[.github/scripts/run-gui-smoke.ps1](../.github/scripts/run-gui-smoke.ps1)
before Rust compilation. The wrapper
launches a hidden browser only on a disposable GitHub-hosted Windows runner,
uses a fresh temporary profile and loopback endpoints, bounds startup/test
duration and stops only its owned processes. Run its `-SelfTest` locally to
check arguments without launching a browser. `node scripts/gui-smoke.cjs
--self-test` checks synthetic preload/version/clipboard/network guards only.

On October 6, the **Test rendered GUI with synthetic data** step passed for
Vellora 0.2.21 source commit
`62b9297390e354e75e4147a7a7b86c8cf2b1b030` in
[Windows CI run 37471723603](https://github.com/Felix561/vellora/actions/runs/37471723603/job/112296641524).
GitHub records the step as successful from 13:34:48 to 13:35:28 UTC. This is a
completed rendered-browser check. The complete run subsequently passed all three
jobs, including actual installer payload/resource verification. The owner-approved
existing-account update to 0.2.21 also passed version, byte, notice and startup
checks; the owner then confirmed the originally reported shortcut sequence works.
See [publication review](PUBLICATION_REVIEW.md) for the separate evidence and limits.

The successful hosted run covered:

- Notebook and Windows Classic on Dashboard, History, AI Models, Settings, and
  Help & About at 1040, 880, and 520 CSS pixels, plus all four guided-setup steps.
- Selected real Chromium keyboard transitions delivered through CDP: Tab,
  Shift+Tab, ArrowRight, and Home. Assertions check the active control,
  `:focus-visible`, a visible outline, and whether it is inside the viewport.
- An approximately 16,000-character synthetic transcript with multiple lines,
  an unbroken URL, Unicode, and literal `<script>` text. It remains untruncated,
  does not execute as HTML, has no horizontal document/content overflow, and
  retains reachable Paste/Delete actions at 880 and 520 CSS pixels.
- Forced-colors emulation with distinguishable focused-button text and visible
  focus, plus reduced-motion emulation on the actual React overlay component
  using mocked Tauri events. Waveform/spinner animations and transitions are
  disabled, with no running animations left.
- 125/150/200 percent viewport/device-pixel-ratio emulation for all five pages
  in both themes, with no horizontal overflow and selected keyboard-focus checks.
- Persistent error feedback, honest settings/key failures, transcript recovery,
  delayed-paste cancellation/single ownership, deletion confirmations and focus,
  history privacy controls, support information exclusions, and setup/practice
  feedback.
- No unhandled promise failures or external network attempts from the main
  window or mocked overlay. External HTTP/WebSocket/API traffic is blocked
  before sending; the browser Clipboard API and Tauri commands are stubbed.

Two harness issues were corrected before this passing run: hosted PATH can
resolve multiple `node.exe` files, so the wrapper now chooses the first normal
PowerShell match; CDP uses modifier bit `8` for Shift, so Shift+Tab now sends
Shift rather than Alt. Both corrections have static self-test assertions.

Viewport/DPR emulation checks frontend reflow; it is not a Windows DPI or
browser-toolbar zoom test. The focused keyboard assertions do not establish
complete traversal of every interactive control. Screen-reader, real browser
zoom, Windows DPI/multi-monitor, native shortcut, physical microphone, paste,
and indicator hit-testing checks remain separate. Passing this hosted step does
not certify a clean Windows installation. The owner's October 6 confirmation
covers the original Ctrl release/re-press while Win remains held; broader native
shortcut and hardware checks remain open.

## README screenshots

The README gallery contains fourteen Vellora 0.2.20 screenshots, captured on
October 4, 2026 at 1240 x 1000 pixels: Dashboard, History, AI Models, General
settings, Appearance, Help & About and Guided setup in Notebook and Windows
Classic. Images live in `docs/images/<page>-<appearance>.png`.

They render the actual React frontend against synthetic Tauri responses in an
isolated screenshot-only browser profile. Transcript wording, counts, charts,
cost estimates and saved-key status are demo fixtures. No real key was entered;
the installed app's credentials, history, settings and clipboard were untouched.
Every image was visually reviewed before adding it, including the replacement
branding. The PNGs have no embedded metadata. The older `images/notebook.png`
and `images/classic.png` filenames now alias the current Appearance captures,
so all sixteen screenshot files use the same reviewed branding. See
[screenshot provenance](images/README.md). These previews verify the illustrated
layout, not native Windows behavior or all display scaling settings.

## Run the test

Run the following commands in PowerShell from the repository root. Keep the Vite
server running in the first terminal:

```powershell
npm ci
npm run dev -- --host 127.0.0.1
```

In a second PowerShell terminal, also from the repository root, launch a hidden
browser with a new temporary profile. Use this profile only for testing:

```powershell
if (Get-NetTCPConnection -LocalPort 19329 -State Listen -ErrorAction SilentlyContinue) {
    throw 'Port 19329 is already in use. Stop the previous test browser first.'
}

$browserCandidates = @(
    (Join-Path $env:ProgramFiles 'Google\Chrome\Application\chrome.exe'),
    (Join-Path ${env:ProgramFiles(x86)} 'Google\Chrome\Application\chrome.exe'),
    (Join-Path $env:ProgramFiles 'Microsoft\Edge\Application\msedge.exe'),
    (Join-Path ${env:ProgramFiles(x86)} 'Microsoft\Edge\Application\msedge.exe')
)
$testBrowserPath = $browserCandidates |
    Where-Object { Test-Path -LiteralPath $_ } |
    Select-Object -First 1
if (-not $testBrowserPath) { throw 'Install Chrome or Edge before running this test.' }

$testProfilePath = Join-Path ([IO.Path]::GetTempPath()) (
    'vellora-gui-' + [guid]::NewGuid().ToString('N')
)
New-Item -ItemType Directory -Path $testProfilePath | Out-Null
$testBrowserArgs = @(
    '--headless=new',
    '--remote-debugging-address=127.0.0.1',
    '--remote-debugging-port=19329',
    ('--user-data-dir="' + $testProfilePath + '"'),
    '--no-first-run',
    '--no-default-browser-check',
    '--disable-background-networking',
    'about:blank'
)
$testBrowserProcess = Start-Process -FilePath $testBrowserPath `
    -ArgumentList $testBrowserArgs -WindowStyle Hidden -PassThru

$testBrowserReady = $false
for ($attempt = 0; $attempt -lt 20; $attempt++) {
    try {
        Invoke-RestMethod 'http://127.0.0.1:19329/json/version' | Out-Null
        $testBrowserReady = $true
        break
    } catch { Start-Sleep -Milliseconds 250 }
}
if (-not $testBrowserReady) { throw 'The test browser did not start on port 19329.' }

node scripts/gui-smoke.cjs
if ($LASTEXITCODE -ne 0) { throw 'GUI smoke test failed. See the assertion above.' }
```

The test creates and closes its own browser tab. It prints `PASS` when its checks
complete and writes synthetic screenshots into the OS temporary directory as
`vellora-audit-<theme>-<case>.png`. Themes are `notebook` and `classic`; cases are
`dashboard`, `settings`, `help-about`, `setup`, `long-transcript`, `forced-colors`,
`200-percent-reflow`, and `reduced-motion-overlay`. These are test evidence,
separate from the reviewed README gallery.

To stop the browser launched in that terminal, use its saved process ID:

```powershell
Stop-Process -Id $testBrowserProcess.Id -ErrorAction SilentlyContinue
```

Stop Vite with **Ctrl+C** in the first terminal. Do not stop all Chrome or Edge
processes; only the isolated browser started for this test needs closing.
