[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$ExecutablePath,

    [Parameter(Mandatory = $true)]
    [string]$ManifestPath,

    [Parameter(Mandatory = $true)]
    [string]$IconPath,

    [Parameter(Mandatory = $true)]
    [string]$LayoutRoot
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

foreach ($requiredPath in @($ExecutablePath, $ManifestPath, $IconPath)) {
    if (-not (Test-Path -LiteralPath $requiredPath -PathType Leaf)) {
        throw "Required development packaging input is missing: $requiredPath"
    }
}

Remove-Item -LiteralPath $LayoutRoot -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $LayoutRoot | Out-Null
Copy-Item -LiteralPath $ExecutablePath -Destination (Join-Path $LayoutRoot "splined.exe")
Copy-Item -LiteralPath $ManifestPath -Destination (Join-Path $LayoutRoot "Package.appxmanifest")
winapp manifest update-assets $IconPath --manifest (Join-Path $LayoutRoot "Package.appxmanifest")
winapp create-debug-identity (Join-Path $LayoutRoot "splined.exe") `
    --manifest (Join-Path $LayoutRoot "Package.appxmanifest") `
    --no-install

$developmentManifest = [xml](Get-Content -Raw -LiteralPath (Join-Path $LayoutRoot "Package.appxmanifest"))
$developmentPackageName = [string]$developmentManifest.Package.Identity.Name
if ([string]::IsNullOrWhiteSpace($developmentPackageName)) {
    throw "The development package identity has no name."
}
$developmentApplicationId = [string]$developmentManifest.Package.Applications.Application.Id
if ([string]::IsNullOrWhiteSpace($developmentApplicationId)) {
    throw "The development package identity has no application ID."
}

$password = [Guid]::NewGuid().ToString("N")
$pfx = Join-Path $env:RUNNER_TEMP "splined-development.pfx"
$cer = [IO.Path]::ChangeExtension($pfx, ".cer")
$developmentPackage = Join-Path $env:RUNNER_TEMP "SPLINED-x64-development.msix"
$thumbprint = $null
$certificateInstalled = $false
$requestPath = $null
$smokePath = $null
try {
    winapp cert generate `
        --manifest (Join-Path $LayoutRoot "Package.appxmanifest") `
        --output $pfx `
        --password $password `
        --export-cer
    $thumbprint = (Get-PfxCertificate -FilePath $cer).Thumbprint
    winapp cert install $cer
    $certificateInstalled = $true
    winapp pack $LayoutRoot `
        --manifest (Join-Path $LayoutRoot "Package.appxmanifest") `
        --executable splined.exe `
        --output $developmentPackage `
        --cert $pfx `
        --cert-password $password
    Add-AppxPackage -Path $developmentPackage
    $package = Get-AppxPackage -Name $developmentPackageName
    if (-not $package -or $package.Architecture -ne "X64") {
        throw "The development MSIX did not install with x64 package identity."
    }
    $installedManifest = $package | Get-AppxPackageManifest
    if ($installedManifest.Package.Applications.Application.Id -ne $developmentApplicationId) {
        throw "The installed package identity is invalid."
    }
    if ($installedManifest.OuterXml -notmatch '<uap:FileType>\.spl</uap:FileType>') {
        throw "The installed package lost its .spl association."
    }

    $smokePath = Join-Path $env:LOCALAPPDATA "Packages\$($package.PackageFamilyName)\LocalState\SPLINED\package-identity-smoke.ok"
    $requestPath = Join-Path $env:LOCALAPPDATA "Packages\$($package.PackageFamilyName)\LocalState\SPLINED\package-identity-smoke.request"
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $smokePath) | Out-Null
    Remove-Item -LiteralPath $smokePath -Force -ErrorAction SilentlyContinue
    Set-Content -LiteralPath $requestPath -Value "requested" -Encoding ascii -NoNewline
    $applicationUserModelId = "$($package.PackageFamilyName)!$developmentApplicationId"
    Start-Process -FilePath explorer.exe -ArgumentList "shell:AppsFolder\$applicationUserModelId" -WindowStyle Hidden
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    while (-not (Test-Path -LiteralPath $smokePath) -and [DateTime]::UtcNow -lt $deadline) {
        Start-Sleep -Milliseconds 200
    }
    if (-not (Test-Path -LiteralPath $smokePath)) {
        throw "The installed Rust/Slint SPLINED process did not confirm package identity and LocalState initialization."
    }
}
finally {
    Get-AppxPackage -Name $developmentPackageName | Remove-AppxPackage -ErrorAction SilentlyContinue
    if ($certificateInstalled -and $thumbprint) {
        certutil -delstore TrustedPeople $thumbprint | Out-Null
    }
    Remove-Item -LiteralPath $pfx, $cer, $developmentPackage -Force -ErrorAction SilentlyContinue
    if ($requestPath) {
        Remove-Item -LiteralPath $requestPath -Force -ErrorAction SilentlyContinue
    }
    if ($smokePath) {
        Remove-Item -LiteralPath $smokePath -Force -ErrorAction SilentlyContinue
    }
    Remove-Item -LiteralPath $LayoutRoot -Recurse -Force -ErrorAction SilentlyContinue
}

