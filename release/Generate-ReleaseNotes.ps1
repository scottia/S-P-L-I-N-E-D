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

$current = [Version]$Version
$numericTags = @(
    foreach ($tag in (& git -C $repository tag --list)) {
        if ($tag -notmatch '^\d+\.\d+\.\d+$' -or [Version]$tag -ge $current) {
            continue
        }
        # A newer-looking tag on another history is not the prior release.
        # Exit 1 means this older numeric tag belongs to another history.
        # PowerShell's native-command error preference must not turn that
        # expected result into a terminating exception.
        $PSNativeCommandUseErrorActionPreference = $false
        try {
            & git -C $repository merge-base --is-ancestor "refs/tags/$tag^{commit}" $releaseCommit
            $ancestorExitCode = $LASTEXITCODE
        } finally {
            $PSNativeCommandUseErrorActionPreference = $true
        }
        if ($ancestorExitCode -eq 0) {
            [PSCustomObject]@{ Text = $tag; Version = [Version]$tag }
        } elseif ($ancestorExitCode -ne 1) {
            throw "Unable to inspect release tag ancestry: $tag"
        }
    }
) | Sort-Object Version -Descending
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
