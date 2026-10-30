[CmdletBinding()]
param(
    [string]$RepositoryRoot = (Split-Path -Parent $PSScriptRoot)
)

$ErrorActionPreference = "Stop"
$testRoot = Join-Path ([IO.Path]::GetTempPath()) ("splined-release-classification-" + [Guid]::NewGuid().ToString("N"))

try {
    & (Join-Path $PSScriptRoot "Generate-ReleaseNotes.ps1") `
        -Version 2.0.12 `
        -RepositoryRoot $RepositoryRoot `
        -ConfigPath (Join-Path $RepositoryRoot "cliff.toml") `
        -GitHubTemplatePath (Join-Path $PSScriptRoot "templates/github-release.tera") `
        -OutputDirectory $testRoot

    $notes = Get-Content -Raw -LiteralPath (Join-Path $testRoot "GITHUB-RELEASE.md")
    if ($notes -notmatch '(?m)^## 🌐 WebGUI / Python$' -or
        $notes -notmatch '(?m)^- \*\*webgui:\*\* Restore complete artist appearance catalog' -or
        $notes -match '(?m)^## 🛍️ Microsoft Store$' -or
        $notes -match '(?m)^## 🪟 Windows$') {
        throw "WebGUI release-note classification regressed into the Store or Windows category."
    }

    Write-Output "WebGUI release-note classification fixture passed."
}
finally {
    if (Test-Path -LiteralPath $testRoot) {
        Remove-Item -LiteralPath $testRoot -Recurse -Force
    }
}
