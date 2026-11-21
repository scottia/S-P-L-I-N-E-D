[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$PackagePath,
    [Parameter(Mandatory = $true)]
    [string]$HighlightsPath,
    [Parameter(Mandatory = $true)]
    [string]$ReleaseContextPath,
    [Parameter(Mandatory = $true)]
    [string]$TargetVersion,
    [Parameter(Mandatory = $true)]
    [string]$TargetCommit,
    [Parameter(Mandatory = $true)]
    [string]$OutputDirectory,
    [string]$ProductId = "9P8G4GMBBVBS"
)

$ErrorActionPreference = "Stop"
$policyPath = Join-Path $PSScriptRoot "StoreHighlightsPolicy.ps1"
if (-not (Test-Path -LiteralPath $policyPath -PathType Leaf)) {
    throw "Microsoft Store Highlights policy is missing: $policyPath"
}
. $policyPath

if ($TargetVersion -notmatch '^\d+\.\d+\.\d+$') {
    throw "TargetVersion must be a numeric SPLINED release version."
}
if ($TargetCommit -notmatch '^[0-9a-fA-F]{40}$') {
    throw "TargetCommit must be a complete 40-character Git commit SHA."
}
if ([String]::IsNullOrWhiteSpace($ProductId)) {
    throw "ProductId is required."
}
foreach ($requiredPath in @($PackagePath, $HighlightsPath, $ReleaseContextPath)) {
    if (-not (Test-Path -LiteralPath $requiredPath -PathType Leaf)) {
        throw "Required local Store review input is missing: $requiredPath"
    }
}

$package = Get-Item -LiteralPath $PackagePath
if ($package.Length -eq 0) {
    throw "The local Store review package is empty."
}
$highlightsSource = (Get-Content -Raw -LiteralPath $HighlightsPath).Trim()
$bounded = ConvertTo-BoundedStoreHighlights -Highlights $highlightsSource -Version $TargetVersion
$releaseContext = @(Get-Content -Raw -LiteralPath $ReleaseContextPath | ConvertFrom-Json)
if (-not ($releaseContext | Where-Object { $_.version -eq $TargetVersion })) {
    throw "Release context does not contain target version $TargetVersion."
}

[IO.Directory]::CreateDirectory($OutputDirectory) | Out-Null
$packageOutput = Join-Path $OutputDirectory "SPLINED-x64-store-unsigned.msix"
$highlightsOutput = Join-Path $OutputDirectory "STORE-HIGHLIGHTS.txt"
$contextOutput = Join-Path $OutputDirectory "release-context.json"
$auditOutput = Join-Path $OutputDirectory "STORE-SUBMISSION.json"

Copy-Item -LiteralPath $package.FullName -Destination $packageOutput -Force
Set-Content -Encoding utf8 -LiteralPath $highlightsOutput -Value $bounded.Highlights
Copy-Item -LiteralPath $ReleaseContextPath -Destination $contextOutput -Force

$audit = [ordered]@{
    schema_version = 1
    review_mode = "local_bundle_only"
    partner_center_contacted = $false
    store_write_performed = $false
    product_id = $ProductId
    target_version = $TargetVersion
    target_commit = $TargetCommit.ToLowerInvariant()
    history_authority = "generated release context; Partner Center not queried"
    package = [ordered]@{
        file_name = [IO.Path]::GetFileName($packageOutput)
        size_bytes = (Get-Item -LiteralPath $packageOutput).Length
        sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $packageOutput).Hash.ToLowerInvariant()
    }
    highlights = [ordered]@{
        file_name = [IO.Path]::GetFileName($highlightsOutput)
        source_characters = $bounded.SourceCharacters
        final_characters = $bounded.FinalCharacters
        compacted = $bounded.Compacted
        candidate_bullets = $bounded.CandidateBullets
        emitted_bullets = $bounded.EmittedBullets
        omitted_bullets = $bounded.OmittedBullets
        details_url = $bounded.DetailsUrl
        sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $highlightsOutput).Hash.ToLowerInvariant()
    }
    release_context = [ordered]@{
        file_name = [IO.Path]::GetFileName($contextOutput)
        sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $contextOutput).Hash.ToLowerInvariant()
    }
    generated_at_utc = [DateTime]::UtcNow.ToString("o")
}
$audit | ConvertTo-Json -Depth 10 | Set-Content -Encoding utf8 -LiteralPath $auditOutput

Write-Output "Created local-only Microsoft Store review bundle for $TargetVersion; Partner Center was not contacted."
