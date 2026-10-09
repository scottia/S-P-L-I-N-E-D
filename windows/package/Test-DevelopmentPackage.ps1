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
    [string]$LayoutRoot
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

foreach ($requiredPath in @($ExecutablePath, $CoreDllPath, $ManifestPath, $IconPath)) {
    if (-not (Test-Path -LiteralPath $requiredPath -PathType Leaf)) {
        throw "Required development packaging input is missing: $requiredPath"
    }
}

Remove-Item -LiteralPath $LayoutRoot -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path (Join-Path $LayoutRoot "runtime") | Out-Null
Copy-Item -LiteralPath $ExecutablePath -Destination (Join-Path $LayoutRoot "splined.exe")
Copy-Item -LiteralPath $CoreDllPath -Destination (Join-Path $LayoutRoot "runtime/splined-core.dll")
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

$password = [Guid]::NewGuid().ToString("N")
$pfx = Join-Path $env:RUNNER_TEMP "splined-development.pfx"
$cer = [IO.Path]::ChangeExtension($pfx, ".cer")
$developmentPackage = Join-Path $env:RUNNER_TEMP "SPLINED-x64-development.msix"
$thumbprint = $null
try {
    winapp cert generate `
        --manifest (Join-Path $LayoutRoot "Package.appxmanifest") `
        --output $pfx `
        --password $password `
        --export-cer
    $thumbprint = (Get-PfxCertificate -FilePath $cer).Thumbprint
    winapp cert install $cer
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
    if ($installedManifest.Package.Applications.Application.Id -ne "SPLINED") {
        throw "The installed package identity is invalid."
    }
    if ($installedManifest.OuterXml -notmatch '<uap:FileType>\.spl</uap:FileType>') {
        throw "The installed package lost its .spl association."
    }

    $smokePath = Join-Path $env:LOCALAPPDATA "Packages\$($package.PackageFamilyName)\LocalState\SPLINED\package-identity-smoke.ok"
    Remove-Item -LiteralPath $smokePath -Force -ErrorAction SilentlyContinue
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public enum SplinedActivateOptions
{
    None = 0,
    DesignMode = 1,
    NoErrorUI = 2,
    NoSplashScreen = 4
}

[ComImport]
[Guid("2e941141-7f97-4756-ba1d-9decde894a3d")]
[InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface ISplinedApplicationActivationManager
{
    int ActivateApplication(
        [MarshalAs(UnmanagedType.LPWStr)] string appUserModelId,
        [MarshalAs(UnmanagedType.LPWStr)] string arguments,
        SplinedActivateOptions options,
        out uint processId);
    int ActivateForFile(
        [MarshalAs(UnmanagedType.LPWStr)] string appUserModelId,
        IntPtr itemArray,
        [MarshalAs(UnmanagedType.LPWStr)] string verb,
        out uint processId);
    int ActivateForProtocol(
        [MarshalAs(UnmanagedType.LPWStr)] string appUserModelId,
        IntPtr itemArray,
        out uint processId);
}

[ComImport]
[Guid("45BA127D-10A8-46EA-8AB7-56EA9078943C")]
public class SplinedApplicationActivationManager {}

public static class SplinedPackageActivator
{
    public static uint Activate(string appUserModelId, string arguments)
    {
        uint processId;
        var manager = (ISplinedApplicationActivationManager)new SplinedApplicationActivationManager();
        int result = manager.ActivateApplication(
            appUserModelId,
            arguments,
            SplinedActivateOptions.NoErrorUI,
            out processId);
        if (result < 0) Marshal.ThrowExceptionForHR(result);
        return processId;
    }
}
'@
    $applicationUserModelId = "$($package.PackageFamilyName)!SPLINED"
    $processId = [SplinedPackageActivator]::Activate($applicationUserModelId, "--package-identity-smoke")
    Wait-Process -Id $processId -Timeout 30 -ErrorAction Stop
    if (-not (Test-Path -LiteralPath $smokePath)) {
        throw "The installed SPLINED process did not confirm package identity and native DLL initialization."
    }
}
finally {
    Get-AppxPackage -Name $developmentPackageName | Remove-AppxPackage -ErrorAction SilentlyContinue
    if ($thumbprint) {
        certutil -delstore TrustedPeople $thumbprint | Out-Null
    }
    Remove-Item -LiteralPath $pfx, $cer, $developmentPackage -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $LayoutRoot -Recurse -Force -ErrorAction SilentlyContinue
}

