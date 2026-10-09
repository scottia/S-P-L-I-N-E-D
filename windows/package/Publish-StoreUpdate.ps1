[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$PackagePath,
    [Parameter(Mandatory = $true)]
    [string]$HighlightsPath,
    [string]$ProductId = "9P8G4GMBBVBS"
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

$requiredSecrets = @(
    "AZURE_AD_APPLICATION_CLIENT_ID",
    "AZURE_AD_APPLICATION_SECRET",
    "AZURE_AD_TENANT_ID",
    "SELLER_ID"
)
$missing = @($requiredSecrets | Where-Object { [String]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($_)) })
if ($missing.Count -gt 0) {
    throw "Required Microsoft Store credentials are unavailable: $($missing -join ', ')"
}

$package = (Resolve-Path -LiteralPath $PackagePath).Path
$highlights = (Resolve-Path -LiteralPath $HighlightsPath).Path
$releaseNotes = (Get-Content -Raw -LiteralPath $highlights).Trim()
if ([String]::IsNullOrWhiteSpace($releaseNotes)) {
    throw "STORE-HIGHLIGHTS.txt is empty."
}

# Stage the package first. Microsoft's top-level publish command creates the
# pending draft, so it must not be called again after the metadata edit.
& msstore reconfigure `
    --tenantId $env:AZURE_AD_TENANT_ID `
    --sellerId $env:SELLER_ID `
    --clientId $env:AZURE_AD_APPLICATION_CLIENT_ID `
    --clientSecret $env:AZURE_AD_APPLICATION_SECRET
& msstore publish $package --appId $ProductId --noCommit

$submissionJson = (& msstore submission get $ProductId) -join "`n"
if ([String]::IsNullOrWhiteSpace($submissionJson)) {
    throw "Microsoft Store returned no pending submission metadata."
}
$submission = $submissionJson | ConvertFrom-Json
$listingsProperty = $submission.PSObject.Properties |
    Where-Object { $_.Name -ieq "Listings" } |
    Select-Object -First 1
if ($null -eq $listingsProperty) { throw "Store submission metadata has no Listings object." }
$englishListingProperty = $listingsProperty.Value.PSObject.Properties |
    Where-Object { $_.Name -ieq "en-US" } |
    Select-Object -First 1
if ($null -eq $englishListingProperty) { throw "Store submission metadata has no en-US listing." }
$baseListingProperty = $englishListingProperty.Value.PSObject.Properties |
    Where-Object { $_.Name -ieq "BaseListing" } |
    Select-Object -First 1
if ($null -eq $baseListingProperty) { throw "Store en-US metadata has no BaseListing object." }
$releaseNotesProperty = $baseListingProperty.Value.PSObject.Properties |
    Where-Object { $_.Name -ieq "ReleaseNotes" } |
    Select-Object -First 1
if ($null -eq $releaseNotesProperty) { throw "Store en-US BaseListing has no ReleaseNotes field." }

# This is the only metadata mutation: every other listing and package field is
# preserved from the pending submission returned after package staging.
$releaseNotesProperty.Value = $releaseNotes
$updatedSubmission = $submission | ConvertTo-Json -Depth 100 -Compress
& msstore submission update $ProductId $updatedSubmission
& msstore submission publish $ProductId
$status = ((& msstore submission status $ProductId) -join "`n").Trim()

if (-not [String]::IsNullOrWhiteSpace($env:GITHUB_STEP_SUMMARY)) {
    @"
## Microsoft Store submission accepted

- Store ID: ``$ProductId``
- Package: ``$([IO.Path]::GetFileName($package))``
- Listing updated: ``en-US / BaseListing / ReleaseNotes`` only
- Package staged before metadata update: ``yes``
- Submission status: ``$status``
"@ | Add-Content -Encoding utf8 $env:GITHUB_STEP_SUMMARY
}

Write-Output "Microsoft Store accepted the SPLINED package and listing update for processing."
