[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$ExecutablePath,

    [Parameter(Mandatory = $true)]
    [string]$CoreDllPath,

    [Parameter(Mandatory = $true)]
    [string]$ManifestPath,

    [Parameter(Mandatory = $true)]
    [string]$IconPath,

    [Parameter(Mandatory = $true)]
    [string]$LayoutRoot,

    [Parameter(Mandatory = $true)]
    [string]$UnpackedRoot,

    [Parameter(Mandatory = $true)]
    [string]$OutputPath,

    [Parameter(Mandatory = $true)]
    [string]$ExpectedVersion
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

function Assert-Amd64Pe([string]$Path) {
    $bytes = [IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $Path).Path)
    if ($bytes.Length -lt 64 -or $bytes[0] -ne 0x4D -or $bytes[1] -ne 0x5A) {
        throw "$Path is not a PE image."
    }
    $peOffset = [BitConverter]::ToInt32($bytes, 0x3C)
    $machine = [BitConverter]::ToUInt16($bytes, $peOffset + 4)
    if ($machine -ne 0x8664) {
        throw "Expected AMD64 PE Machine 0x8664 for $Path, found 0x$($machine.ToString('X4'))."
    }
}

foreach ($requiredPath in @($ExecutablePath, $CoreDllPath, $ManifestPath, $IconPath)) {
    if (-not (Test-Path -LiteralPath $requiredPath -PathType Leaf)) {
        throw "Required Store packaging input is missing: $requiredPath"
    }
}
if ($ExpectedVersion -notmatch '^\d+\.\d+\.\d+\.0$') {
    throw "ExpectedVersion must be a four-part Store version ending in .0."
}

Remove-Item -LiteralPath $LayoutRoot, $UnpackedRoot -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path (Join-Path $LayoutRoot "runtime") | Out-Null
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $OutputPath) | Out-Null
Copy-Item -LiteralPath $ExecutablePath -Destination (Join-Path $LayoutRoot "splined.exe")
Copy-Item -LiteralPath $CoreDllPath -Destination (Join-Path $LayoutRoot "runtime/splined-core.dll")
Copy-Item -LiteralPath $ManifestPath -Destination (Join-Path $LayoutRoot "Package.appxmanifest")
winapp manifest update-assets $IconPath --manifest (Join-Path $LayoutRoot "Package.appxmanifest")

# This production layout is packed directly from the Partner Center identity.
# Debug identity and development signing are prohibited in this script.
winapp pack $LayoutRoot `
    --manifest (Join-Path $LayoutRoot "Package.appxmanifest") `
    --executable splined.exe `
    --output $OutputPath `
    --no-sign
winapp tool makeappx unpack /p $OutputPath /d $UnpackedRoot /o

$manifest = [xml](Get-Content -Raw -LiteralPath (Join-Path $UnpackedRoot "AppxManifest.xml"))
$namespace = New-Object Xml.XmlNamespaceManager($manifest.NameTable)
$namespace.AddNamespace("f", "http://schemas.microsoft.com/appx/manifest/foundation/windows10")
$namespace.AddNamespace("uap", "http://schemas.microsoft.com/appx/manifest/uap/windows10")
$namespace.AddNamespace("rescap", "http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities")
$identity = $manifest.SelectSingleNode("/f:Package/f:Identity", $namespace)
$publisherDisplayName = $manifest.SelectSingleNode("/f:Package/f:Properties/f:PublisherDisplayName", $namespace)
$displayName = $manifest.SelectSingleNode("/f:Package/f:Properties/f:DisplayName", $namespace)
$targetFamily = $manifest.SelectSingleNode("/f:Package/f:Dependencies/f:TargetDeviceFamily", $namespace)
$application = $manifest.SelectSingleNode("/f:Package/f:Applications/f:Application", $namespace)
$fileType = $manifest.SelectSingleNode("//uap:FileType[text()='.spl']", $namespace)
$fullTrust = $manifest.SelectSingleNode("/f:Package/f:Capabilities/rescap:Capability[@Name='runFullTrust']", $namespace)

if ($identity.Name -cne "Psycotix.SPLINED") {
    throw "Store MSIX Identity.Name is not the Partner Center value."
}
if ($identity.Publisher -cne "CN=FE370EF6-D95D-4A6F-9AAB-2654E6DE00FE") {
    throw "Store MSIX Identity.Publisher is not the Partner Center value."
}
if ($identity.Version -cne $ExpectedVersion) {
    throw "Store MSIX version '$($identity.Version)' does not match '$ExpectedVersion'."
}
if ($identity.ProcessorArchitecture -cne "x64") {
    throw "Store MSIX processor architecture is not x64."
}
if ($publisherDisplayName.InnerText -cne "Psycotix") {
    throw "Store MSIX PublisherDisplayName is not Psycotix."
}
if ($displayName.InnerText -cne "SPLINED") {
    throw "Store MSIX DisplayName is not SPLINED."
}
if ($targetFamily.Name -cne "Windows.Desktop") {
    throw "Store MSIX TargetDeviceFamily is not Windows.Desktop."
}
if ($targetFamily.MinVersion -cne "10.0.17763.0") {
    throw "Store MSIX Windows.Desktop MinVersion must remain 10.0.17763.0."
}
if ($targetFamily.MaxVersionTested -cne "10.0.26100.0") {
    throw "Store MSIX MaxVersionTested must remain 10.0.26100.0."
}
if ($application.Id -cne "SPLINED") {
    throw "Store MSIX Application Id changed unexpectedly."
}
if (-not $fileType) {
    throw "Store MSIX does not declare the .spl file association."
}
if (-not $fullTrust) {
    throw "Store MSIX does not declare runFullTrust."
}
if (Test-Path -LiteralPath (Join-Path $UnpackedRoot "AppxSignature.p7x")) {
    throw "The Partner Center Store MSIX must remain unsigned."
}

$storeExe = Join-Path $UnpackedRoot "splined.exe"
$storeDll = Join-Path $UnpackedRoot "runtime/splined-core.dll"
if (-not (Test-Path -LiteralPath $storeExe -PathType Leaf)) {
    throw "Store MSIX is missing splined.exe."
}
if (-not (Test-Path -LiteralPath $storeDll -PathType Leaf)) {
    throw "Store MSIX is missing runtime/splined-core.dll."
}
Assert-Amd64Pe $storeExe
Assert-Amd64Pe $storeDll

[pscustomobject]@{
    Name = [string]$identity.Name
    Publisher = [string]$identity.Publisher
    PublisherDisplayName = [string]$publisherDisplayName.InnerText
    DisplayName = [string]$displayName.InnerText
    Version = [string]$identity.Version
    ProcessorArchitecture = [string]$identity.ProcessorArchitecture
    TargetDeviceFamily = [string]$targetFamily.Name
    MinVersion = [string]$targetFamily.MinVersion
    MaxVersionTested = [string]$targetFamily.MaxVersionTested
    ApplicationId = [string]$application.Id
    PackagePath = (Resolve-Path -LiteralPath $OutputPath).Path
}

