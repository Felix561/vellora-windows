param(
    [Parameter(Mandatory = $true)][string]$InstallerPath,
    [Parameter(Mandatory = $true)][string]$ExpectedExecutablePath,
    [Parameter(Mandatory = $true)][string]$ReportPath
)

# Archive inspection only. Never invoke the NSIS installer or extracted payload.
# GitHub's Windows runner provides 7-Zip: https://github.com/actions/runner-images/blob/main/images/windows/Windows2022-Readme.md
# 7-Zip supports unpacking NSIS: https://www.7-zip.org/
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Get-RequiredFileHash([string]$FilePath, [string]$Description) {
    if (-not (Test-Path -LiteralPath $FilePath -PathType Leaf)) {
        throw "Required $Description file is missing."
    }
    return (Get-FileHash -LiteralPath $FilePath -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Find-UniquePayloadFile([string]$Directory, [string]$FileName) {
    $files = @(Get-ChildItem -LiteralPath $Directory -Recurse -File |
        Where-Object Name -eq $FileName)
    if ($files.Count -ne 1) {
        throw "Expected exactly one $FileName in the installer payload."
    }
    if (($files[0].Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw 'Installer payload file must not be a reparse point.'
    }
    return $files[0].FullName
}

try {
    $repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
    $expectedExecutableHash = Get-RequiredFileHash $ExpectedExecutablePath 'privacy-scanned executable'
    $installerHash = Get-RequiredFileHash $InstallerPath 'installer'
    $installerName = [IO.Path]::GetFileName($InstallerPath)
    if ($installerName -notmatch '^Vellora_[0-9]+\.[0-9]+\.[0-9]+_x64-setup\.exe$') {
        throw 'Review installer name must identify a Vellora x64 semantic version.'
    }

    $sevenZipCommand = Get-Command 7z.exe -ErrorAction SilentlyContinue
    $sevenZip = if ($sevenZipCommand) { $sevenZipCommand.Source } else {
        Join-Path $env:ProgramFiles '7-Zip\7z.exe'
    }
    if (-not (Test-Path -LiteralPath $sevenZip -PathType Leaf)) {
        throw '7-Zip is required to verify the installer without running it.'
    }

    $temporaryRoot = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [IO.Path]::GetTempPath() }
    $extractionRoot = Join-Path $temporaryRoot ('vellora-installer-review-' + [guid]::NewGuid().ToString('N'))
    [void](New-Item -ItemType Directory -Path $extractionRoot)
    # Tool output can contain workspace/user roots. Keep it out of CI logs.
    $extractionOutput = & $sevenZip x '-y' '-bd' '-bb0' "-o$extractionRoot" $InstallerPath 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw '7-Zip could not safely inspect the NSIS review installer.'
    }
    $extractionOutput = $null

    $payloadExecutable = Find-UniquePayloadFile $extractionRoot 'vellora.exe'
    $payloadExecutableHash = Get-RequiredFileHash $payloadExecutable 'extracted executable'
    # Tauri CLI 2.11.2 patches UNK -> NSS for the installer, then restores the
    # original EXE. The helper requires full byte equality with that one exact
    # reviewed transformation and directly privacy-scans the extracted EXE.
    $helperArguments = @(
        (Join-Path $PSScriptRoot 'check-packaged-executable.mjs'),
        '--compiled-exe', $ExpectedExecutablePath,
        '--payload-exe', $payloadExecutable,
        '--forbid-path', $repositoryRoot
    )
    $cargoRoot = if ($env:CARGO_HOME) { $env:CARGO_HOME } else {
        Join-Path ([Environment]::GetFolderPath('UserProfile')) '.cargo'
    }
    $helperArguments += @('--forbid-path', $cargoRoot)
    if ($env:VELLORA_RUST_SYSROOT) {
        $helperArguments += @('--forbid-path', $env:VELLORA_RUST_SYSROOT)
    }
    # A native stderr message must not bypass the redacting outer catch under
    # Windows PowerShell. The helper itself emits only static errors/safe JSON.
    $previousErrorActionPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $helperOutput = @(& node @helperArguments 2>&1)
        $helperExitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previousErrorActionPreference
    }
    $jsonLines = @($helperOutput | ForEach-Object { $_.ToString().Trim() } | Where-Object { $_.StartsWith('{') })
    if ($helperExitCode -ne 0) {
        # The optional failure report contains hashes, sizes and a difference
        # offset only. Do not print stderr or source-byte excerpts.
        if ($jsonLines.Count -eq 1) {
            $failure = $jsonLines[0] | ConvertFrom-Json
            if ($failure.PSObject.Properties.Name -contains 'firstDifferentByteOffset') {
                [ordered]@{
                    executableMatchesReviewedNsisPatch = $false
                    compiledExecutableSha256 = $failure.compiledExecutableSha256
                    expectedPatchedExecutableSha256 = $failure.expectedPatchedExecutableSha256
                    payloadExecutableSha256 = $failure.payloadExecutableSha256
                    compiledExecutableBytes = $failure.compiledExecutableBytes
                    payloadExecutableBytes = $failure.payloadExecutableBytes
                    firstDifferentByteOffset = $failure.firstDifferentByteOffset
                } | ConvertTo-Json -Compress | Write-Output
            }
        }
        throw 'Installer executable failed exact NSIS patch or extracted-payload privacy verification.'
    }
    if ($jsonLines.Count -ne 1) {
        throw 'Packaged executable verifier did not return exactly one safe report.'
    }
    $executableVerification = $jsonLines[0] | ConvertFrom-Json
    if ($executableVerification.compiledExecutableSha256 -ne $expectedExecutableHash -or
        $executableVerification.payloadExecutableSha256 -ne $payloadExecutableHash -or
        $executableVerification.expectedPatchedExecutableSha256 -ne $payloadExecutableHash -or
        $executableVerification.executableMatchesReviewedNsisPatch -ne $true -or
        $executableVerification.payloadPrivacyPassed -ne $true) {
        throw 'Packaged executable verification report does not match the inspected files.'
    }

    $noticeHashes = [ordered]@{}
    foreach ($fileName in @('LICENSE', 'THIRD_PARTY_NOTICES.md', 'THIRD_PARTY_NOTICES_NATIVE.md')) {
        $sourceHash = Get-RequiredFileHash (Join-Path $repositoryRoot $fileName) 'committed notice'
        $payloadFile = Find-UniquePayloadFile $extractionRoot $fileName
        $payloadHash = Get-RequiredFileHash $payloadFile 'bundled notice'
        if ($payloadHash -ne $sourceHash) {
            throw "Installer $fileName does not match its committed notice."
        }
        $noticeHashes[$fileName] = $payloadHash
    }

    $reportDirectory = [IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($ReportPath))
    if (-not (Test-Path -LiteralPath $reportDirectory -PathType Container)) {
        throw 'The verification report directory must already exist.'
    }
    $report = [ordered]@{
        schemaVersion = 2
        installer = $installerName
        installerSha256 = $installerHash
        compiledExecutableSha256 = $expectedExecutableHash
        expectedPatchedExecutableSha256 = $executableVerification.expectedPatchedExecutableSha256
        payloadExecutableSha256 = $payloadExecutableHash
        compiledExecutableBytes = $executableVerification.compiledExecutableBytes
        payloadExecutableBytes = $executableVerification.payloadExecutableBytes
        executableMatchesReviewedNsisPatch = $true
        bundleTypePatch = $executableVerification.bundleTypePatch
        payloadPrivacyPassed = $true
        payloadPrivacyFindingCounts = $executableVerification.payloadPrivacyFindingCounts
        notices = $noticeHashes
        verificationMethod = '7-Zip archive extraction; installer and payload were never executed'
    } | ConvertTo-Json -Depth 5
    $report | Set-Content -LiteralPath $ReportPath -Encoding utf8
    $report | Write-Output
    Write-Output 'Review installer verified: exact NSIS-patched executable, payload privacy, and all three bundled notices match.'
    # Isolated extraction data belongs to runner TEMP and expires with the job.
    # Do not recursively delete computed paths on a contributor machine.
} catch {
    # Even an OS/tool exception could contain a personal root. Report only our
    # static messages; preserve any inspected files for local diagnosis.
    $message = $_.Exception.Message
    if ($message -match '^(Required (privacy-scanned executable|installer|extracted executable|committed notice|bundled notice) file is missing\.|Expected exactly one (vellora\.exe|LICENSE|THIRD_PARTY_NOTICES\.md|THIRD_PARTY_NOTICES_NATIVE\.md) in the installer payload\.|Installer payload file must not be a reparse point\.|Review installer name must identify a Vellora x64 semantic version\.|7-Zip is required to verify the installer without running it\.|7-Zip could not safely inspect the NSIS review installer\.|Installer executable failed exact NSIS patch or extracted-payload privacy verification\.|Packaged executable verifier did not return exactly one safe report\.|Packaged executable verification report does not match the inspected files\.|Installer (LICENSE|THIRD_PARTY_NOTICES\.md|THIRD_PARTY_NOTICES_NATIVE\.md) does not match its committed notice\.|The verification report directory must already exist\.)$') {
        [Console]::Error.WriteLine($message)
    } else {
        [Console]::Error.WriteLine('Installer verification failed; tool output and local paths are omitted.')
    }
    exit 1
}
