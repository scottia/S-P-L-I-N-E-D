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

$current = [Version]$Version
$numericTags = @(& git -C $repository tag --list) |
    Where-Object { $_ -match '^\d+\.\d+\.\d+$' } |
    ForEach-Object { [PSCustomObject]@{ Text = $_; Version = [Version]$_ } } |
    Where-Object { $_.Version -lt $current } |
    Sort-Object Version -Descending
if ($numericTags.Count -eq 0) {
    throw "No previous numeric release tag exists before $Version."
}
$previousTag = $numericTags[0].Text
$range = "$previousTag..$Version"

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
    "context_path=$contextPath" | Add-Content -Encoding utf8 $env:GITHUB_OUTPUT
    "github_notes_path=$githubNotesPath" | Add-Content -Encoding utf8 $env:GITHUB_OUTPUT
}

Write-Output "Generated incremental git-cliff GitHub release notes for $range"
