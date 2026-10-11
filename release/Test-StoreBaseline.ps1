[CmdletBinding()]
param(
    [string]$RepositoryRoot = (Split-Path -Parent $PSScriptRoot)
)

$ErrorActionPreference = "Stop"
$resolver = Join-Path $PSScriptRoot "Resolve-StoreBaseline.ps1"
$generator = Join-Path $PSScriptRoot "Generate-StoreHighlights.ps1"
$config = Join-Path (Split-Path -Parent $PSScriptRoot) "cliff.toml"
$template = Join-Path $PSScriptRoot "templates/store-highlights.tera"
$fixtures = Join-Path $PSScriptRoot "tests"
$testRoot = Join-Path ([IO.Path]::GetTempPath()) ("splined-store-baseline-" + [Guid]::NewGuid().ToString("N"))

function Assert-ThrowsLike {
    param(
        [Parameter(Mandatory = $true)]
        [scriptblock]$Action,
        [Parameter(Mandatory = $true)]
        [string]$Pattern
    )

    try {
        & $Action
    }
    catch {
        if ($_.Exception.Message -notmatch $Pattern) {
            throw "Expected failure matching '$Pattern'; received '$($_.Exception.Message)'."
        }
        return
    }
    throw "Expected failure matching '$Pattern', but the command succeeded."
}

try {
    [IO.Directory]::CreateDirectory($testRoot) | Out-Null
    $positive = Join-Path $testRoot "positive"
    & $resolver `
        -TargetVersion 1.0.78 `
        -RepositoryRoot $RepositoryRoot `
        -OutputDirectory $positive `
        -ApplicationPath (Join-Path $fixtures "store-app-published.json") `
        -SubmissionPath (Join-Path $fixtures "store-submission-1.0.71.json")

    $metadata = Get-Content -Raw -LiteralPath (Join-Path $positive "STORE-BASELINE.json") | ConvertFrom-Json
    if ($metadata.baseline_version -cne "1.0.71" -or
        $metadata.target_version -cne "1.0.78" -or
        $metadata.range -cne "1.0.71..1.0.78" -or
        $metadata.store_status -cne "Published") {
        throw "Published Store fixture did not resolve the expected 1.0.71..1.0.78 range."
    }

    & $generator `
        -Version 1.0.78 `
        -BaselineMetadataPath (Join-Path $positive "STORE-BASELINE.json") `
        -RepositoryRoot $RepositoryRoot `
        -ConfigPath $config `
        -StoreTemplatePath $template `
        -OutputDirectory $positive
    $highlights = (Get-Content -Raw -LiteralPath (Join-Path $positive "STORE-HIGHLIGHTS.txt")).Trim()
    if ([regex]::Matches($highlights, "(?m)^What's new in 1\.0\.78:$").Count -ne 1 -or
        $highlights -match '(?im)^- Merge ' -or
        $highlights -match '(?im)^- Prepare ' -or
        $highlights -notmatch '(?im)^- Use semantic versions for updates$' -or
        $highlights -notmatch '(?im)^- Unify artwork editing layout$' -or
        $highlights.Length -gt 1500) {
        throw "Cumulative Store Highlights were incomplete, noisy, repeated, or over the Store limit."
    }
    $audit = Get-Content -Raw -LiteralPath (Join-Path $positive "STORE-SUBMISSION.json") | ConvertFrom-Json
    if ($audit.range -cne "1.0.71..1.0.78" -or $audit.highlights_characters -ne $highlights.Length) {
        throw "Store submission audit does not describe the rendered cumulative history."
    }

    Assert-ThrowsLike -Pattern "active or unresolved submission" -Action {
        & $resolver `
            -TargetVersion 1.0.78 `
            -RepositoryRoot $RepositoryRoot `
            -OutputDirectory (Join-Path $testRoot "pending") `
            -ApplicationPath (Join-Path $fixtures "store-app-pending.json") `
            -SubmissionPath (Join-Path $fixtures "store-submission-1.0.71.json")
    }

    Assert-ThrowsLike -Pattern "must be newer" -Action {
        & $resolver `
            -TargetVersion 1.0.71 `
            -RepositoryRoot $RepositoryRoot `
            -OutputDirectory (Join-Path $testRoot "same") `
            -ApplicationPath (Join-Path $fixtures "store-app-published.json") `
            -SubmissionPath (Join-Path $fixtures "store-submission-1.0.71.json")
    }

    $mixed = Join-Path $testRoot "mixed"
    & $resolver `
        -TargetVersion 1.0.78 `
        -RepositoryRoot $RepositoryRoot `
        -OutputDirectory $mixed `
        -ApplicationPath (Join-Path $fixtures "store-app-published.json") `
        -SubmissionPath (Join-Path $fixtures "store-submission-mixed.json")
    $mixedMetadata = Get-Content -Raw -LiteralPath (Join-Path $mixed "STORE-BASELINE.json") | ConvertFrom-Json
    if ($mixedMetadata.baseline_version -cne "1.0.71") {
        throw "Store history did not select the newest active x64 package from a multi-package published submission."
    }

    Write-Output "Store baseline and cumulative git-cliff fixture tests passed."
}
finally {
    if (Test-Path -LiteralPath $testRoot) {
        Remove-Item -LiteralPath $testRoot -Recurse -Force
    }
}
