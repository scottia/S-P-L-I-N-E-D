[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$TargetVersion,
    [Parameter(Mandatory = $true)]
    [string]$RepositoryRoot,
    [string]$ProductId = "9P8G4GMBBVBS",
    [Parameter(Mandatory = $true)]
    [string]$OutputDirectory,
    [string]$ApplicationPath,
    [string]$SubmissionPath
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

if ($TargetVersion -notmatch '^\d+\.\d+\.\d+$') {
    throw "TargetVersion must be an existing numeric release tag."
}
if ([String]::IsNullOrWhiteSpace($ProductId)) {
    throw "ProductId is required."
}

$repository = (Resolve-Path -LiteralPath $RepositoryRoot).Path
& git -C $repository rev-parse --verify "refs/tags/$TargetVersion^{commit}" | Out-Null
if ($LASTEXITCODE -ne 0) { throw "Target release tag does not exist: $TargetVersion" }

function Get-CaseInsensitivePropertyValue {
    param(
        [Parameter(Mandatory = $true)]
        [object]$Object,
        [Parameter(Mandatory = $true)]
        [string]$Name
    )

    if ($null -eq $Object) { return $null }
    $property = $Object.PSObject.Properties |
        Where-Object { $_.Name -ieq $Name } |
        Select-Object -First 1
    if ($null -eq $property) { return $null }
    return $property.Value
}

function Find-ApplicationPackages {
    param([object]$Value)

    if ($null -eq $Value) { return $null }
    if ($Value -is [string] -or $Value.GetType().IsPrimitive) { return $null }

    $packages = Get-CaseInsensitivePropertyValue -Object $Value -Name "applicationPackages"
    if ($null -ne $packages) { return @($packages) }

    if ($Value -is [System.Collections.IEnumerable] -and $Value -isnot [pscustomobject]) {
        foreach ($item in $Value) {
            $found = Find-ApplicationPackages -Value $item
            if ($null -ne $found) { return @($found) }
        }
        return $null
    }

    foreach ($property in $Value.PSObject.Properties) {
        $found = Find-ApplicationPackages -Value $property.Value
        if ($null -ne $found) { return @($found) }
    }
    return $null
}

$usingFixtures = -not [String]::IsNullOrWhiteSpace($ApplicationPath) -or
    -not [String]::IsNullOrWhiteSpace($SubmissionPath)
if ($usingFixtures -and ([String]::IsNullOrWhiteSpace($ApplicationPath) -or [String]::IsNullOrWhiteSpace($SubmissionPath))) {
    throw "ApplicationPath and SubmissionPath must be supplied together."
}

if ($usingFixtures) {
    $applicationText = Get-Content -Raw -LiteralPath $ApplicationPath
} else {
    # `apps get` and `submission get` are the Store CLI's machine-readable
    # JSON boundaries. Avoid parsing the human-facing `submission status`
    # output, whose stream and formatting are not a stable data contract.
    $applicationText = (& msstore apps get $ProductId) -join "`n"
}
if ([String]::IsNullOrWhiteSpace($applicationText)) {
    throw "Microsoft Store returned no application history."
}
try {
    $application = $applicationText | ConvertFrom-Json
}
catch {
    throw "Microsoft Store returned invalid application JSON: $($_.Exception.Message)"
}

$pendingSubmission = Get-CaseInsensitivePropertyValue -Object $application -Name "pendingApplicationSubmission"
if ($null -ne $pendingSubmission) {
    $pendingId = [string](Get-CaseInsensitivePropertyValue -Object $pendingSubmission -Name "id")
    if ([String]::IsNullOrWhiteSpace($pendingId)) { $pendingId = "unknown" }
    throw "Microsoft Store has an active or unresolved submission ($pendingId). Refusing to replace it or derive highlights from a pending draft."
}
$lastPublishedSubmission = Get-CaseInsensitivePropertyValue -Object $application -Name "lastPublishedApplicationSubmission"
$lastPublishedSubmissionId = [string](Get-CaseInsensitivePropertyValue -Object $lastPublishedSubmission -Name "id")
if ([String]::IsNullOrWhiteSpace($lastPublishedSubmissionId)) {
    throw "Microsoft Store returned no last-published submission authority."
}

if ($usingFixtures) {
    $submissionText = Get-Content -Raw -LiteralPath $SubmissionPath
} else {
    $submissionText = (& msstore submission get $ProductId) -join "`n"
}
if ([String]::IsNullOrWhiteSpace($submissionText)) {
    throw "Microsoft Store returned no last-published submission metadata."
}

try {
    $submission = $submissionText | ConvertFrom-Json
}
catch {
    throw "Microsoft Store returned invalid submission JSON: $($_.Exception.Message)"
}

$submissionId = [string](Get-CaseInsensitivePropertyValue -Object $submission -Name "id")
if ($submissionId -cne $lastPublishedSubmissionId) {
    throw "Microsoft Store submission metadata does not match lastPublishedApplicationSubmission ($lastPublishedSubmissionId)."
}

$packages = @(Find-ApplicationPackages -Value $submission)
if ($packages.Count -eq 0) {
    throw "The last-published Store submission has no applicationPackages array."
}

$activeX64Packages = @(
    $packages |
        Where-Object {
            $architecture = [string](Get-CaseInsensitivePropertyValue -Object $_ -Name "architecture")
            $fileStatus = [string](Get-CaseInsensitivePropertyValue -Object $_ -Name "fileStatus")
            $architecture -ieq "x64" -and $fileStatus -ine "PendingDelete"
        }
)
if ($activeX64Packages.Count -eq 0) {
    throw "The last-published Store submission has no active x64 package."
}

$packageVersions = @(
    $activeX64Packages |
        ForEach-Object { [string](Get-CaseInsensitivePropertyValue -Object $_ -Name "version") } |
        ForEach-Object { $_.Trim() } |
        Where-Object { $_ -match '^\d+\.\d+\.\d+\.0$' } |
        Sort-Object -Unique
)
if ($packageVersions.Count -eq 0) {
    throw "The last-published Store submission has no SPLINED x.y.z.0 x64 package version."
}
$packageVersion = @($packageVersions | Sort-Object { [Version]$_ })[-1]
$match = [regex]::Match($packageVersion, '^(\d+)\.(\d+)\.(\d+)\.0$')
$baselineVersion = "$($match.Groups[1].Value).$($match.Groups[2].Value).$($match.Groups[3].Value)"

& git -C $repository rev-parse --verify "refs/tags/$baselineVersion^{commit}" | Out-Null
if ($LASTEXITCODE -ne 0) {
    throw "Last-published Store version has no matching numeric Git tag: $baselineVersion"
}
if ([Version]$baselineVersion -ge [Version]$TargetVersion) {
    throw "Target Store version $TargetVersion must be newer than the last-published Store version $baselineVersion."
}

$range = "$baselineVersion..$TargetVersion"
[IO.Directory]::CreateDirectory($OutputDirectory) | Out-Null
$metadataPath = Join-Path $OutputDirectory "STORE-BASELINE.json"
$metadata = [ordered]@{
    schema_version = 1
    product_id = $ProductId
    store_status = "Published"
    last_published_submission_id = $lastPublishedSubmissionId
    baseline_version = $baselineVersion
    baseline_package_version = $packageVersion
    target_version = $TargetVersion
    range = $range
    resolved_at_utc = [DateTime]::UtcNow.ToString("o")
}
$metadata | ConvertTo-Json -Depth 10 | Set-Content -Encoding utf8 -LiteralPath $metadataPath

if (-not [String]::IsNullOrWhiteSpace($env:GITHUB_OUTPUT)) {
    "baseline_version=$baselineVersion" | Add-Content -Encoding utf8 $env:GITHUB_OUTPUT
    "baseline_package_version=$packageVersion" | Add-Content -Encoding utf8 $env:GITHUB_OUTPUT
    "target_version=$TargetVersion" | Add-Content -Encoding utf8 $env:GITHUB_OUTPUT
    "range=$range" | Add-Content -Encoding utf8 $env:GITHUB_OUTPUT
    "metadata_path=$metadataPath" | Add-Content -Encoding utf8 $env:GITHUB_OUTPUT
}

Write-Output "Resolved last-published Microsoft Store range $range (Store package $packageVersion)."
