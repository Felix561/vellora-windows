param([switch]$PersistForGitHubActions)

$ErrorActionPreference = 'Stop'
if ($env:RUSTFLAGS -and -not $env:CARGO_ENCODED_RUSTFLAGS) {
    throw 'Move existing Rust flags to CARGO_ENCODED_RUSTFLAGS before configuring release path remapping.'
}

$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..')).TrimEnd('\', '/')
$cargoRoot = if ($env:CARGO_HOME) {
    [IO.Path]::GetFullPath($env:CARGO_HOME)
} else {
    Join-Path ([Environment]::GetFolderPath('UserProfile')) '.cargo'
}
$sysroot = (& rustc --print sysroot | Out-String).Trim()
if ($LASTEXITCODE -ne 0 -or -not $sysroot) {
    throw 'Unable to determine the Rust sysroot for release path remapping.'
}

$mappings = @(
    @{ Source = $repositoryRoot; Destination = '/vellora' },
    @{ Source = $cargoRoot.TrimEnd('\', '/'); Destination = '/cargo' },
    @{ Source = $sysroot.TrimEnd('\', '/'); Destination = '/rust' }
) | Sort-Object { $_.Source.Length }
$flags = foreach ($mapping in $mappings) {
    foreach ($source in @($mapping.Source, $mapping.Source.Replace('\', '/')) | Select-Object -Unique) {
        "--remap-path-prefix=$source=$($mapping.Destination)"
    }
}
$separator = [char]31
$encoded = $flags -join $separator
if ($env:CARGO_ENCODED_RUSTFLAGS) {
    $encoded = $env:CARGO_ENCODED_RUSTFLAGS + $separator + $encoded
}
$env:CARGO_ENCODED_RUSTFLAGS = $encoded
$env:CARGO_HOME = $cargoRoot
$env:VELLORA_RUST_SYSROOT = $sysroot

if ($PersistForGitHubActions) {
    if (-not $env:GITHUB_ENV) {
        throw 'GitHub Actions environment persistence was requested outside a workflow.'
    }
    @(
        "CARGO_ENCODED_RUSTFLAGS=$encoded",
        "CARGO_HOME=$cargoRoot",
        "VELLORA_RUST_SYSROOT=$sysroot"
    ) | Add-Content -LiteralPath $env:GITHUB_ENV -Encoding utf8
}
Write-Output 'Configured compiler path remapping; release binaries still require the privacy scan.'
