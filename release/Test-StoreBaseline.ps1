[CmdletBinding()]
param(
    [string]$RepositoryRoot = (Split-Path -Parent $PSScriptRoot)
)

$ErrorActionPreference = "Stop"
$resolver = Join-Path $PSScriptRoot "Resolve-StoreBaseline.ps1"
$generator = Join-Path $PSScriptRoot "Generate-StoreHighlights.ps1"
$policy = Join-Path $PSScriptRoot "StoreHighlightsPolicy.ps1"
$config = Join-Path (Split-Path -Parent $PSScriptRoot) "cliff.toml"
$template = Join-Path $PSScriptRoot "templates/store-highlights.tera"
$fixtures = Join-Path $PSScriptRoot "tests"
$testRoot = Join-Path ([IO.Path]::GetTempPath()) ("splined-store-baseline-" + [Guid]::NewGuid().ToString("N"))

. $policy

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
    $syntheticBullets = @(1..80 | ForEach-Object { "- Customer-facing release improvement $($_.ToString('00')) with complete wording." })
    $syntheticSource = (@("What's new in 9.9.9:") + $syntheticBullets) -join "`n"
    $bounded = ConvertTo-BoundedStoreHighlights -Highlights $syntheticSource -Version "9.9.9"
    $boundedLines = @($bounded.Highlights -split "`n")
    $emittedSynthetic = @($boundedLines | Where-Object { $_ -like "- Customer-facing release improvement*" })
    if (-not $bounded.Compacted -or
        $bounded.FinalCharacters -gt 1500 -or
        $bounded.SourceCharacters -le 1500 -or
        $bounded.CandidateBullets -ne 80 -or
        $bounded.EmittedBullets -ne $emittedSynthetic.Count -or
        $bounded.OmittedBullets -ne (80 - $emittedSynthetic.Count) -or
        $bounded.Highlights -notmatch '(?m)^- Full release details: https://github\.com/scottia/S-P-L-I-N-E-D/releases/tag/9\.9\.9$') {
        throw "Over-limit Store Highlights were not compacted at complete bullet boundaries with an audited release link."
    }
    for ($index = 0; $index -lt $emittedSynthetic.Count; $index++) {
        if ($emittedSynthetic[$index] -cne $syntheticBullets[$index]) {
            throw "Store Highlights compaction did not preserve the priority-ordered bullet prefix."
        }
    }

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
    if ($audit.range -cne "1.0.71..1.0.78" -or
        $audit.highlights_characters -ne $highlights.Length -or
        $audit.highlights_source_characters -lt $audit.highlights_characters -or
        $null -eq $audit.highlights_compacted -or
        $audit.highlights_omitted_bullets -lt 0) {
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

    $readOnlyPending = Join-Path $testRoot "pending-read-only"
    & $resolver `
        -TargetVersion 1.0.78 `
        -RepositoryRoot $RepositoryRoot `
        -OutputDirectory $readOnlyPending `
        -ApplicationPath (Join-Path $fixtures "store-app-pending.json") `
        -SubmissionPath (Join-Path $fixtures "store-submission-1.0.71.json") `
        -AllowPendingSubmission
    $readOnlyMetadata = Get-Content -Raw -LiteralPath (Join-Path $readOnlyPending "STORE-BASELINE.json") | ConvertFrom-Json
    if ($readOnlyMetadata.baseline_version -cne "1.0.71" -or
        $readOnlyMetadata.target_version -cne "1.0.78") {
        throw "Read-only review mode did not retain the last published Store baseline while a pending submission existed."
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
