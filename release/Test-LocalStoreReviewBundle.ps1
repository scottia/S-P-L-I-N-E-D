[CmdletBinding()]
param(
    [string]$RepositoryRoot = (Split-Path -Parent $PSScriptRoot)
)

$ErrorActionPreference = "Stop"
$builder = Join-Path $PSScriptRoot "New-LocalStoreReviewBundle.ps1"
$testRoot = Join-Path ([IO.Path]::GetTempPath()) ("splined-local-store-review-" + [Guid]::NewGuid().ToString("N"))

try {
    [IO.Directory]::CreateDirectory($testRoot) | Out-Null
    $package = Join-Path $testRoot "candidate.msix"
    $highlights = Join-Path $testRoot "highlights.txt"
    $context = Join-Path $testRoot "release-context.json"
    $output = Join-Path $testRoot "bundle"
    [IO.File]::WriteAllBytes($package, [byte[]](1, 2, 3, 4, 5))
    Set-Content -Encoding utf8 -LiteralPath $highlights -Value "What's new in 3.0.22:`n- Local Store review packaging."
    @([ordered]@{ version = "3.0.22"; commits = @() }) |
        ConvertTo-Json -Depth 10 |
        Set-Content -Encoding utf8 -LiteralPath $context

    & $builder `
        -PackagePath $package `
        -HighlightsPath $highlights `
        -ReleaseContextPath $context `
        -TargetVersion "3.0.22" `
        -TargetCommit "0123456789abcdef0123456789abcdef01234567" `
        -OutputDirectory $output

    foreach ($file in @(
        "SPLINED-x64-store-unsigned.msix",
        "STORE-HIGHLIGHTS.txt",
        "STORE-SUBMISSION.json",
        "release-context.json"
    )) {
        $path = Join-Path $output $file
        if (-not (Test-Path -LiteralPath $path -PathType Leaf) -or (Get-Item -LiteralPath $path).Length -eq 0) {
            throw "Local Store review bundle is missing $file."
        }
    }

    $audit = Get-Content -Raw -LiteralPath (Join-Path $output "STORE-SUBMISSION.json") | ConvertFrom-Json
    if ($audit.review_mode -cne "local_bundle_only" -or
        $audit.partner_center_contacted -ne $false -or
        $audit.store_write_performed -ne $false -or
        $audit.target_version -cne "3.0.22" -or
        $audit.package.sha256 -cne (Get-FileHash -Algorithm SHA256 -LiteralPath $package).Hash.ToLowerInvariant()) {
        throw "Local Store review audit does not prove a non-contacting, non-writing package."
    }

    Write-Output "Local Store review bundle tests passed."
}
finally {
    if (Test-Path -LiteralPath $testRoot) {
        Remove-Item -LiteralPath $testRoot -Recurse -Force
    }
}
