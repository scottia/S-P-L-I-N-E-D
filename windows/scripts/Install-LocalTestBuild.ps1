[CmdletBinding()]
param(
    [string]$InstallPath = 'E:\apps\SPLINED\splined.exe',
    [switch]$Quick,
    [switch]$NoLaunch
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$repositoryRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..')).Path
$manifestPath = Join-Path $repositoryRoot 'windows\Cargo.toml'
$targetRoot = [System.IO.Path]::GetFullPath(
    (Join-Path $repositoryRoot 'windows\target\x86_64-pc-windows-msvc')
).TrimEnd('\')
$releaseRoot = [System.IO.Path]::GetFullPath((Join-Path $targetRoot 'release'))
$buildPath = Join-Path $releaseRoot 'splined.exe'
$installDirectory = Split-Path -Parent $InstallPath

if (-not (Test-Path -LiteralPath $installDirectory -PathType Container)) {
    throw "The local SPLINED install directory does not exist: $installDirectory"
}

$resolvedInstallDirectory = (Resolve-Path -LiteralPath $installDirectory).Path.TrimEnd('\')
$resolvedInstallPath = [System.IO.Path]::GetFullPath($InstallPath)
if ([System.IO.Path]::GetDirectoryName($resolvedInstallPath).TrimEnd('\') -ne $resolvedInstallDirectory) {
    throw "The install target escaped its expected directory: $resolvedInstallPath"
}

$running = @(Get-CimInstance Win32_Process | Where-Object {
    $_.Name -ieq 'splined.exe'
})
if ($running.Count -gt 0) {
    $locations = ($running | ForEach-Object { $_.ExecutablePath } | Where-Object { $_ }) -join ', '
    throw "Close SPLINED before applying a local test build. Running: $locations"
}

if ([System.IO.Path]::GetDirectoryName($releaseRoot).TrimEnd('\') -ne $targetRoot) {
    throw "The clean release target escaped its expected directory: $releaseRoot"
}

$buildKind = if ($Quick) { 'quick incremental' } else { 'clean production-equivalent' }
Write-Host "Building a $buildKind MEDIAPC test executable..." -ForegroundColor Cyan
Push-Location $repositoryRoot
$previousIncremental = $env:CARGO_INCREMENTAL
try {
    if ($Quick) {
        # Cargo still validates dependencies and relinks the complete executable;
        # it simply reuses unchanged release objects for faster local UI testing.
        $env:CARGO_INCREMENTAL = '1'
    }
    else {
        # Match the GitHub Windows release build: discard target-specific release
        # output and prohibit incremental code generation before rebuilding.
        if (Test-Path -LiteralPath $releaseRoot -PathType Container) {
            Remove-Item -LiteralPath $releaseRoot -Recurse -Force
        }
        $env:CARGO_INCREMENTAL = '0'
    }
    & cargo +1.99.0 build `
        --manifest-path $manifestPath `
        --locked `
        --release `
        --target x86_64-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) {
        throw "The local Windows test build failed with exit code $LASTEXITCODE."
    }
}
finally {
    if ($null -eq $previousIncremental) {
        Remove-Item Env:CARGO_INCREMENTAL -ErrorAction SilentlyContinue
    }
    else {
        $env:CARGO_INCREMENTAL = $previousIncremental
    }
    Pop-Location
}

$resolvedBuildPath = (Resolve-Path -LiteralPath $buildPath).Path
$expectedBuildRoot = (Resolve-Path -LiteralPath $releaseRoot).Path.TrimEnd('\')
if ([System.IO.Path]::GetDirectoryName($resolvedBuildPath).TrimEnd('\') -ne $expectedBuildRoot) {
    throw "The build output escaped its expected directory: $resolvedBuildPath"
}

$stream = [System.IO.File]::OpenRead($resolvedBuildPath)
try {
    $signature = @($stream.ReadByte(), $stream.ReadByte())
}
finally {
    $stream.Dispose()
}
if ($signature[0] -ne 0x4D -or $signature[1] -ne 0x5A) {
    throw 'The local build output is not a Windows executable.'
}

$builtHash = (Get-FileHash -LiteralPath $resolvedBuildPath -Algorithm SHA256).Hash
Copy-Item -LiteralPath $resolvedBuildPath -Destination $resolvedInstallPath -Force
$installedHash = (Get-FileHash -LiteralPath $resolvedInstallPath -Algorithm SHA256).Hash
if ($builtHash -ne $installedHash) {
    throw 'The installed executable does not match the completed local test build.'
}

Write-Host "Installed $buildKind local test build: $resolvedInstallPath" -ForegroundColor Green
Write-Host "SHA-256: $installedHash" -ForegroundColor Green
Write-Host 'Git branches, tags, releases, and Microsoft Store state were not changed.' -ForegroundColor Yellow

if (-not $NoLaunch) {
    Start-Process -FilePath $resolvedInstallPath
    Write-Host 'SPLINED started for local testing.' -ForegroundColor Green
}
