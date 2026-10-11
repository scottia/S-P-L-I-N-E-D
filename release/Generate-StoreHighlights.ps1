[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Version,
    [Parameter(Mandatory = $true)]
    [string]$BaselineMetadataPath,
    [Parameter(Mandatory = $true)]
    [string]$RepositoryRoot,
    [Parameter(Mandatory = $true)]
    [string]$ConfigPath,
    [Parameter(Mandatory = $true)]
    [string]$StoreTemplatePath,
    [Parameter(Mandatory = $true)]
    [string]$OutputDirectory
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

if ($Version -notmatch '^\d+\.\d+\.\d+$') {
    throw "Version must be an existing numeric release tag."
}
$repository = (Resolve-Path -LiteralPath $RepositoryRoot).Path
foreach ($requiredPath in @($BaselineMetadataPath, $ConfigPath, $StoreTemplatePath)) {
    if (-not (Test-Path -LiteralPath $requiredPath -PathType Leaf)) {
        throw "Required Store highlight input is missing: $requiredPath"
    }
}

$baseline = Get-Content -Raw -LiteralPath $BaselineMetadataPath | ConvertFrom-Json
if ($baseline.schema_version -ne 1 -or $baseline.target_version -cne $Version) {
    throw "Store baseline metadata does not match target version $Version."
}
if ($baseline.baseline_version -notmatch '^\d+\.\d+\.\d+$' -or
    $baseline.range -cne "$($baseline.baseline_version)..$Version") {
    throw "Store baseline metadata contains an invalid release range."
}
if ($baseline.store_status -ine "Published") {
    throw "Store highlights require a last-published Store baseline."
}

foreach ($tag in @($baseline.baseline_version, $Version)) {
    & git -C $repository rev-parse --verify "refs/tags/$tag^{commit}" | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Store highlight tag does not exist: $tag" }
}

[IO.Directory]::CreateDirectory($OutputDirectory) | Out-Null
$contextPath = Join-Path $OutputDirectory "store-context.json"
$highlightsPath = Join-Path $OutputDirectory "STORE-HIGHLIGHTS.txt"
$auditPath = Join-Path $OutputDirectory "STORE-SUBMISSION.json"

# Store history can span several GitHub-only numeric tags. Ignore those tag
# boundaries while building this context so the Store receives one cumulative
# target-version body rather than one repeated body per intervening tag.
& git-cliff --config $ConfigPath --repository $repository --ignore-tags '.*' --tag $Version --context --output $contextPath $baseline.range
& git-cliff --config $ConfigPath --from-context $contextPath --body-file $StoreTemplatePath --strip all --output $highlightsPath

foreach ($output in @($contextPath, $highlightsPath)) {
    if (-not (Test-Path -LiteralPath $output -PathType Leaf) -or (Get-Item -LiteralPath $output).Length -eq 0) {
        throw "git-cliff produced an empty Store highlight output: $output"
    }
}

$highlights = (Get-Content -Raw -LiteralPath $highlightsPath).Trim()
if ($highlights.Length -gt 1500) {
    throw "Store Highlights are $($highlights.Length) characters; Microsoft Store What's new is limited to 1500. Refine customer-facing commit messages instead of silently truncating release history."
}
Set-Content -Encoding utf8 -LiteralPath $highlightsPath -Value $highlights

$context = Get-Content -Raw -LiteralPath $contextPath | ConvertFrom-Json
$releases = @($context | Where-Object { $null -ne $_ })
if (-not ($releases | Where-Object { $_.version -eq $Version })) {
    throw "git-cliff Store context does not contain release $Version."
}

$targetCommit = (& git -C $repository rev-parse "refs/tags/$Version^{commit}").Trim()
$baselineCommit = (& git -C $repository rev-parse "refs/tags/$($baseline.baseline_version)^{commit}").Trim()
$audit = [ordered]@{
    schema_version = 1
    product_id = $baseline.product_id
    store_status = $baseline.store_status
    baseline_version = $baseline.baseline_version
    baseline_package_version = $baseline.baseline_package_version
    baseline_commit = $baselineCommit
    target_version = $Version
    target_commit = $targetCommit
    range = $baseline.range
    release_note_engine = "git-cliff 2.14.2"
    classification = "cliff.toml"
    highlights_characters = $highlights.Length
    highlights_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $highlightsPath).Hash.ToLowerInvariant()
    generated_at_utc = [DateTime]::UtcNow.ToString("o")
}
$audit | ConvertTo-Json -Depth 10 | Set-Content -Encoding utf8 -LiteralPath $auditPath

if (-not [String]::IsNullOrWhiteSpace($env:GITHUB_OUTPUT)) {
    "baseline_version=$($baseline.baseline_version)" | Add-Content -Encoding utf8 $env:GITHUB_OUTPUT
    "range=$($baseline.range)" | Add-Content -Encoding utf8 $env:GITHUB_OUTPUT
    "context_path=$contextPath" | Add-Content -Encoding utf8 $env:GITHUB_OUTPUT
    "store_highlights_path=$highlightsPath" | Add-Content -Encoding utf8 $env:GITHUB_OUTPUT
    "audit_path=$auditPath" | Add-Content -Encoding utf8 $env:GITHUB_OUTPUT
}

Write-Output "Generated cumulative Microsoft Store Highlights for $($baseline.range) ($($highlights.Length)/1500 characters)."
