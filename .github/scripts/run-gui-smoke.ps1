param([switch]$SelfTest)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# This wrapper launches browsers only on disposable GitHub-hosted Windows CI.
# Local static validation is permitted through -SelfTest; it launches nothing.
function Get-TestBrowserCandidates {
    param([string]$ProgramFilesRoot, [string]$ProgramFilesX86Root)
    @(
        (Join-Path $ProgramFilesRoot 'Google\Chrome\Application\chrome.exe'),
        (Join-Path $ProgramFilesX86Root 'Google\Chrome\Application\chrome.exe'),
        (Join-Path $ProgramFilesRoot 'Microsoft\Edge\Application\msedge.exe'),
        (Join-Path $ProgramFilesX86Root 'Microsoft\Edge\Application\msedge.exe')
    )
}

function Get-TestBrowserArguments {
    param([string]$ProfilePath)
    @(
        '--headless=new',
        '--remote-debugging-address=127.0.0.1',
        '--remote-debugging-port=19329',
        ('--user-data-dir="' + $ProfilePath + '"'),
        '--no-first-run',
        '--no-default-browser-check',
        '--disable-extensions',
        '--disable-background-networking',
        '--disable-component-update',
        '--disable-sync',
        'about:blank'
    )
}

function Get-TestNodePath {
    param([object[]]$Applications)
    # Hosted PATH can resolve more than one node.exe. Start-Process requires one
    # executable string; preserve PowerShell's normal first-match PATH priority.
    $application = $Applications | Select-Object -First 1
    if ($null -eq $application -or $application.Source -isnot [string] -or [string]::IsNullOrWhiteSpace($application.Source)) {
        throw 'A single Node executable could not be resolved for the GUI test.'
    }
    return $application.Source
}

function Test-LoopbackPort {
    param([int]$Port)
    $client = [Net.Sockets.TcpClient]::new()
    try {
        $connect = $client.ConnectAsync('127.0.0.1', $Port)
        return ($connect.Wait(250) -and $client.Connected)
    } catch { return $false }
    finally { $client.Dispose() }
}

function Wait-TestEndpoint {
    param([string]$Uri, [Diagnostics.Process]$Process)
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    while ([DateTime]::UtcNow -lt $deadline) {
        if ($Process.HasExited) { throw 'An isolated GUI test process exited before it was ready.' }
        try {
            Invoke-WebRequest -Uri $Uri -TimeoutSec 1 -UseBasicParsing | Out-Null
            return
        } catch { Start-Sleep -Milliseconds 250 }
    }
    throw 'An isolated GUI test endpoint did not become ready within 30 seconds.'
}

function Get-SafeTestDiagnostic {
    param([string]$Message)
    if ([string]::IsNullOrWhiteSpace($Message)) { return '(no diagnostic message)' }
    # This runs only against synthetic hosted-test diagnostics. Strip terminal
    # escapes, URLs, and paths before truncating; an unquoted path can contain
    # spaces, so redact its entire remaining line rather than leaking fragments.
    $safe = $Message -replace '\x1B\[[0-?]*[ -/]*[@-~]', ''
    $safe = $safe -replace '(?i)\b[a-z][a-z0-9+.-]*://[^\s"''<>]+', '[redacted-url]'
    $safe = $safe -replace '"[A-Za-z]:[\\/][^"]*"|''[A-Za-z]:[\\/][^'']*''', '[redacted-path]'
    $safe = $safe -replace '(?im)[A-Za-z]:[\\/][^\r\n]*|\\\\[^\r\n]*|(?<![\w:])/[A-Za-z_~][^\r\n]*', '[redacted-path]'
    $safe = $safe -replace '"[^"\r\n]*[\\/][^"\r\n]*"|''[^''\r\n]*[\\/][^''\r\n]*''', '[redacted-path]'
    $safe = $safe -replace '(?i)\bsk-[a-z0-9_-]{8,}|\b(api[_-]?key|token|password|secret)\s*[:=]\s*[^\s,;]+', '[redacted-secret]'
    $safe = ($safe -replace '[\x00-\x1F\x7F]', ' ').Trim()
    if ($safe.Length -gt 320) { $safe = $safe.Substring(0, 320) + ' [truncated]' }
    return $safe
}

function Write-BoundedTestDiagnostics {
    param([string]$Label, [string]$LogPath)
    if (-not (Test-Path -LiteralPath $LogPath -PathType Leaf)) { return }
    try {
        $lines = @(Get-Content -LiteralPath $LogPath -Tail 30 |
            Where-Object { $_ -match '(?i)error|failed|cannot|could not|eaddrinuse|enoent|eacces|not found|unknown|invalid|unsupported' } |
            Select-Object -First 6)
        foreach ($line in $lines) {
            Write-Output ("GUI runner diagnostic ({0}): {1}" -f $Label, (Get-SafeTestDiagnostic -Message $line))
        }
    } catch {
        Write-Output ("GUI runner diagnostic ({0}): Diagnostic log could not be read." -f $Label)
    }
}

function Stop-TestProcess {
    param([Diagnostics.Process]$Process)
    if ($null -eq $Process) { return }
    try {
        if (-not $Process.HasExited) {
            # Only a process returned by this script's Start-Process is eligible.
            Stop-Process -Id $Process.Id -ErrorAction Stop
            $null = $Process.WaitForExit(5000)
        }
    } catch { Write-Warning 'An owned GUI test process could not be stopped; the hosted runner will dispose it.' }
}

function Close-OwnedTestBrowser {
    param([Diagnostics.Process]$Process)
    if ($null -eq $Process -or $Process.HasExited) { return }
    $socket = [Net.WebSockets.ClientWebSocket]::new()
    $timeout = [Threading.CancellationTokenSource]::new(1500)
    try {
        # A graceful close lets Chromium shut down its child processes. This
        # endpoint is used only while our saved browser process is still alive;
        # startup refused any preexisting listener on this dedicated CI port.
        $version = Invoke-RestMethod 'http://127.0.0.1:19329/json/version' -TimeoutSec 1
        if ($version.webSocketDebuggerUrl -notmatch '^ws://127\.0\.0\.1:19329/devtools/browser/[a-zA-Z0-9-]+$') { return }
        $socket.ConnectAsync([Uri]$version.webSocketDebuggerUrl, $timeout.Token).GetAwaiter().GetResult()
        $command = [Text.Encoding]::UTF8.GetBytes('{"id":1,"method":"Browser.close"}')
        $socket.SendAsync([ArraySegment[byte]]::new($command), [Net.WebSockets.WebSocketMessageType]::Text, $true, $timeout.Token).GetAwaiter().GetResult()
        $null = $Process.WaitForExit(5000)
    } catch {
        # The root-PID fallback below is restricted to this owned process.
    } finally {
        $socket.Dispose()
        $timeout.Dispose()
    }
}

if ($SelfTest) {
    $exampleProfile = Join-Path ([IO.Path]::GetTempPath()) 'vellora synthetic profile'
    $arguments = @(Get-TestBrowserArguments -ProfilePath $exampleProfile)
    if ($arguments -notcontains '--remote-debugging-address=127.0.0.1') { throw 'Self-test: browser debugger must be bound to loopback.' }
    if ($arguments -notcontains '--remote-debugging-port=19329') { throw 'Self-test: unexpected browser debugger port.' }
    if ($arguments -notcontains ('--user-data-dir="' + $exampleProfile + '"')) { throw 'Self-test: isolated profile argument must preserve quoting.' }
    if ($arguments -contains '--no-sandbox') { throw 'Self-test: browser sandbox must remain enabled.' }
    $candidates = @(Get-TestBrowserCandidates -ProgramFilesRoot 'C:\Example Programs' -ProgramFilesX86Root 'C:\Example Programs x86')
    if ($candidates.Count -ne 4 -or @($candidates | Where-Object { $_ -notmatch '\\(chrome|msedge)\.exe$' }).Count) { throw 'Self-test: browser discovery must use official installed executables.' }
    $nodeCandidates = @(
        [pscustomobject]@{ Source = 'C:\Example Toolcache\node.exe' },
        [pscustomobject]@{ Source = 'C:\Example Programs\node.exe' }
    )
    $resolvedNode = Get-TestNodePath -Applications $nodeCandidates
    if ($resolvedNode -isnot [string] -or $resolvedNode -cne $nodeCandidates[0].Source) { throw 'Self-test: multiple Node candidates must resolve to one executable in PATH priority.' }
    $diagnostics = @(
        'Error: Cannot find module ''D:\Example Runner\node_modules\vite\bin\vite.js''',
        'Failed to load C:\Example Programs\synthetic project\vite.config.ts',
        'Error at \\example-host\synthetic share\script.js',
        'Failed loading /tmp/synthetic project/vite.config.ts',
        'Request failed https://example.invalid/test?token=synthetic ws://example.invalid/socket',
        'Error loading ''node_modules/vite/bin/vite.js''',
        'Error api_key=synthetic-credential sk-syntheticcredential'
    )
    foreach ($diagnostic in $diagnostics) {
        $safe = Get-SafeTestDiagnostic -Message $diagnostic
        if ($safe -match 'Example|Runner\\|example-host|synthetic share|/tmp|example\.invalid|node_modules/|synthetic-credential|sk-synthetic') {
            throw 'Self-test: diagnostic redaction left synthetic private content visible.'
        }
        if ($safe -notmatch 'Error|Failed|Request') { throw 'Self-test: diagnostic redaction must retain the failure category.' }
    }
    if ((Get-SafeTestDiagnostic -Message ('x' * 1000)).Length -gt 340) { throw 'Self-test: diagnostic output must be bounded.' }
    Write-Output 'PASS: GUI runner wrapper argument/discovery/redaction self-test; no process launched.'
    exit 0
}

if ($env:GITHUB_ACTIONS -cne 'true' -or $env:RUNNER_OS -cne 'Windows' -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted') {
    throw 'GUI browser launching is restricted to an isolated GitHub-hosted Windows runner. Use -SelfTest locally.'
}

$workspace = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$nodePath = Get-TestNodePath -Applications @(Get-Command node -CommandType Application)
$vitePath = Join-Path $workspace 'node_modules\vite\bin\vite.js'
$smokePath = Join-Path $workspace 'scripts\gui-smoke.cjs'
if (-not (Test-Path -LiteralPath $vitePath -PathType Leaf) -or -not (Test-Path -LiteralPath $smokePath -PathType Leaf)) {
    throw 'GUI test prerequisites are missing. Install locked frontend dependencies first.'
}
if ((Test-LoopbackPort -Port 1420) -or (Test-LoopbackPort -Port 19329)) {
    throw 'A reserved GUI test port is already occupied. The test will not attach to an existing process.'
}
$browserPath = @(Get-TestBrowserCandidates -ProgramFilesRoot $env:ProgramFiles -ProgramFilesX86Root ${env:ProgramFiles(x86)}) |
    Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } |
    Select-Object -First 1
if (-not $browserPath) { throw 'The hosted Windows runner has no supported Chrome or Edge installation.' }

$tempBase = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [IO.Path]::GetTempPath() }
$testRoot = Join-Path $tempBase ('vellora-gui-' + [guid]::NewGuid().ToString('N'))
$profilePath = Join-Path $testRoot 'browser-profile'
New-Item -ItemType Directory -Path $profilePath -Force | Out-Null
$viteProcess = $null
$browserProcess = $null
$smokeProcess = $null
$smokeOutput = Join-Path $testRoot 'smoke.stdout.log'
$smokeError = Join-Path $testRoot 'smoke.stderr.log'
$viteOutput = Join-Path $testRoot 'vite.stdout.log'
$viteError = Join-Path $testRoot 'vite.stderr.log'
$testStage = 'Vite launch'

try {
    $viteProcess = Start-Process -FilePath $nodePath -WorkingDirectory $workspace `
        -ArgumentList @(('"' + $vitePath + '"'), '--host', '127.0.0.1', '--port', '1420', '--strictPort') `
        -WindowStyle Hidden -PassThru `
        -RedirectStandardOutput $viteOutput -RedirectStandardError $viteError
    $testStage = 'Vite readiness'
    Wait-TestEndpoint -Uri 'http://127.0.0.1:1420/' -Process $viteProcess
    $testStage = 'isolated browser launch'
    $browserProcess = Start-Process -FilePath $browserPath -WorkingDirectory $workspace `
        -ArgumentList (Get-TestBrowserArguments -ProfilePath $profilePath) `
        -WindowStyle Hidden -PassThru `
        -RedirectStandardOutput (Join-Path $testRoot 'browser.stdout.log') `
        -RedirectStandardError (Join-Path $testRoot 'browser.stderr.log')
    $testStage = 'isolated browser readiness'
    Wait-TestEndpoint -Uri 'http://127.0.0.1:19329/json/version' -Process $browserProcess
    $testStage = 'rendered assertions'
    $smokeProcess = Start-Process -FilePath $nodePath -WorkingDirectory $workspace `
        -ArgumentList @(('"' + $smokePath + '"')) -WindowStyle Hidden -PassThru `
        -RedirectStandardOutput $smokeOutput -RedirectStandardError $smokeError
    if (-not $smokeProcess.WaitForExit(180000)) { throw 'Rendered GUI smoke checks exceeded their 180-second bound.' }

    # Emit only safe PASS/assertion messages, without raw logs or stack traces.
    $messages = @()
    if (Test-Path -LiteralPath $smokeOutput) { $messages += Get-Content -LiteralPath $smokeOutput }
    if (Test-Path -LiteralPath $smokeError) { $messages += Get-Content -LiteralPath $smokeError }
    foreach ($message in $messages) {
        if ($message -match '^(PASS:|GUI smoke failed:)') {
            $safeMessage = Get-SafeTestDiagnostic -Message $message
            Write-Output $safeMessage
        }
    }
    if ($smokeProcess.ExitCode -ne 0) { throw 'Rendered GUI smoke checks failed. See the safe assertion message above.' }
    if (-not @($messages | Where-Object { $_ -like 'PASS:*' }).Count) { throw 'Rendered GUI smoke checks exited without a success marker.' }
} catch {
    $failure = $_
    $category = $failure.CategoryInfo.Category.ToString()
    $exceptionType = $failure.Exception.GetType().Name
    Write-Output ("GUI runner diagnostic: {0}; category={1}; exception={2}; message={3}" -f
        $testStage, $category, $exceptionType, (Get-SafeTestDiagnostic -Message $failure.Exception.Message))
    if ($testStage -like 'Vite*') {
        if ($null -ne $viteProcess) {
            try {
                $viteProcess.Refresh()
                if ($viteProcess.HasExited) { Write-Output ("GUI runner diagnostic: Vite process exit code={0}." -f $viteProcess.ExitCode) }
            } catch { Write-Output 'GUI runner diagnostic: Vite process status could not be read.' }
        }
        Write-BoundedTestDiagnostics -Label 'Vite stderr' -LogPath $viteError
        Write-BoundedTestDiagnostics -Label 'Vite stdout' -LogPath $viteOutput
    }
    throw "GUI runner failed during $testStage (category=$category; exception=$exceptionType). See the bounded redacted diagnostics above."
} finally {
    Stop-TestProcess -Process $smokeProcess
    Close-OwnedTestBrowser -Process $browserProcess
    Stop-TestProcess -Process $browserProcess
    Stop-TestProcess -Process $viteProcess
    # No recursive deletion: the disposable runner owns this temporary profile.
}
