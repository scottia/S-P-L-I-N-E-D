[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Version,
    [Parameter(Mandatory = $true)]
    [string]$RepositoryRoot,
    [Parameter(Mandatory = $true)]
    [string]$ConfigPath,
    [Parameter(Mandatory = $true)]
    [string]$GitHubTemplatePath,
    [Parameter(Mandatory = $true)]
    [string]$OutputDirectory
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

if ($Version -notmatch '^\d+\.\d+\.\d+$') {
    throw "Version must be an existing numeric release tag."
}

$repository = (Resolve-Path -LiteralPath $RepositoryRoot).Path
foreach ($requiredPath in @($ConfigPath, $GitHubTemplatePath)) {
    if (-not (Test-Path -LiteralPath $requiredPath -PathType Leaf)) {
        throw "Required git-cliff input is missing: $requiredPath"
    }
}

& git -C $repository rev-parse --verify "refs/tags/$Version^{commit}" | Out-Null
if ($LASTEXITCODE -ne 0) { throw "Release tag does not exist: $Version" }
$releaseCommit = (& git -C $repository rev-parse "refs/tags/$Version^{commit}").Trim()
if ($LASTEXITCODE -ne 0) { throw "Unable to resolve release tag: $Version" }

$resolverPath = Join-Path $repository ".github/scripts/release_note_baseline.py"
if (-not (Test-Path -LiteralPath $resolverPath -PathType Leaf)) {
    throw "Release-note baseline resolver is missing: $resolverPath"
}
$pythonCommand = if ([String]::IsNullOrWhiteSpace($env:SPLINED_PYTHON)) {
    "python3"
} else {
    $env:SPLINED_PYTHON
}
$resolvedJson = (& $pythonCommand $resolverPath --repository $repository --version $Version) -join "`n"
if ($LASTEXITCODE -ne 0) {
    throw "Release-note baseline resolution failed for $Version."
}
$resolved = $resolvedJson | ConvertFrom-Json
$previousTag = $resolved.previous_tag
$baselineCommit = $resolved.baseline_commit
$range = $resolved.range
$baselineMode = $resolved.mode
if ([String]::IsNullOrWhiteSpace($previousTag) -or
    [String]::IsNullOrWhiteSpace($baselineCommit) -or
    [String]::IsNullOrWhiteSpace($range) -or
    [String]::IsNullOrWhiteSpace($baselineMode)) {
    throw "Release-note baseline resolver returned incomplete evidence for $Version."
}

[IO.Directory]::CreateDirectory($OutputDirectory) | Out-Null
$contextPath = Join-Path $OutputDirectory "release-context.json"
$githubNotesPath = Join-Path $OutputDirectory "GITHUB-RELEASE.md"

& git-cliff --config $ConfigPath --repository $repository --context --output $contextPath $range
& git-cliff --config $ConfigPath --from-context $contextPath --body-file $GitHubTemplatePath --strip all --output $githubNotesPath

foreach ($output in @($contextPath, $githubNotesPath)) {
    if (-not (Test-Path -LiteralPath $output -PathType Leaf) -or (Get-Item -LiteralPath $output).Length -eq 0) {
        throw "git-cliff produced an empty release-note output: $output"
    }
}

$context = Get-Content -Raw -LiteralPath $contextPath | ConvertFrom-Json
$releases = @($context | Where-Object { $null -ne $_ })
if (-not ($releases | Where-Object { $_.version -eq $Version })) {
    throw "git-cliff context does not contain release $Version."
}

if (-not [String]::IsNullOrWhiteSpace($env:GITHUB_OUTPUT)) {
    "previous_tag=$previousTag" | Add-Content -Encoding utf8 $env:GITHUB_OUTPUT
    "baseline_commit=$baselineCommit" | Add-Content -Encoding utf8 $env:GITHUB_OUTPUT
    "baseline_mode=$baselineMode" | Add-Content -Encoding utf8 $env:GITHUB_OUTPUT
    "context_path=$contextPath" | Add-Content -Encoding utf8 $env:GITHUB_OUTPUT
    "github_notes_path=$githubNotesPath" | Add-Content -Encoding utf8 $env:GITHUB_OUTPUT
}

Write-Output "Generated incremental git-cliff GitHub release notes for $range ($baselineMode; previous tag $previousTag)"
