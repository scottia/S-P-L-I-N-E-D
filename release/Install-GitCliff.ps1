[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$InstallDirectory
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$version = "2.14.2"
$linuxSha256 = "24f397c733add5390fdceee3a2088588ab0d5f944ce00d34cb7029b888cf2db4"
$windowsSha256 = "6ece2112b3f4af462190ecbea4aeb1315fff6af415629b597f84319073c31131"
$isWindowsHost = $PSVersionTable.Platform -eq "Win32NT" -or $env:OS -eq "Windows_NT"

if ($isWindowsHost) {
    $archiveName = "git-cliff-$version-x86_64-pc-windows-msvc.zip"
    $expectedSha256 = $windowsSha256
    $binaryName = "git-cliff.exe"
} else {
    $archiveName = "git-cliff-$version-x86_64-unknown-linux-gnu.tar.gz"
    $expectedSha256 = $linuxSha256
    $binaryName = "git-cliff"
}

$downloadRoot = Join-Path ([IO.Path]::GetTempPath()) "splined-git-cliff-$version"
$archivePath = Join-Path $downloadRoot $archiveName
$extractRoot = Join-Path $downloadRoot "extract"
[IO.Directory]::CreateDirectory($downloadRoot) | Out-Null
if (Test-Path -LiteralPath $extractRoot) {
    Remove-Item -LiteralPath $extractRoot -Recurse -Force
}
[IO.Directory]::CreateDirectory($extractRoot) | Out-Null

$url = "https://github.com/orhun/git-cliff/releases/download/v$version/$archiveName"
Invoke-WebRequest -Uri $url -OutFile $archivePath
$actualSha256 = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
if ($actualSha256 -cne $expectedSha256) {
    throw "git-cliff $version archive SHA-256 validation failed."
}

if ($isWindowsHost) {
    Expand-Archive -LiteralPath $archivePath -DestinationPath $extractRoot -Force
} else {
    & tar -xzf $archivePath -C $extractRoot
    if ($LASTEXITCODE -ne 0) { throw "Unable to extract git-cliff $version." }
}

$matches = @([IO.Directory]::EnumerateFiles($extractRoot, $binaryName, [IO.SearchOption]::AllDirectories))
if ($matches.Count -ne 1) {
    throw "Expected exactly one $binaryName in the git-cliff $version archive."
}

[IO.Directory]::CreateDirectory($InstallDirectory) | Out-Null
$installedBinary = Join-Path $InstallDirectory $binaryName
Copy-Item -LiteralPath $matches[0] -Destination $installedBinary -Force
if (-not $isWindowsHost) {
    & chmod +x $installedBinary
    if ($LASTEXITCODE -ne 0) { throw "Unable to mark git-cliff executable." }
}

$reportedVersion = (& $installedBinary --version) -join "`n"
if ($reportedVersion.Trim() -cne "git-cliff $version") {
    throw "Expected git-cliff $version; received '$($reportedVersion.Trim())'."
}

if (-not [String]::IsNullOrWhiteSpace($env:GITHUB_PATH)) {
    $InstallDirectory | Add-Content -Encoding utf8 $env:GITHUB_PATH
}

Write-Output $installedBinary
