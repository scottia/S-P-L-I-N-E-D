@echo off
setlocal
pushd "%~dp0"
set "QA_ROOT=%~dp0..\..\target-windows\winforms-qa"
set "QA_OUT=%QA_ROOT%\preview"
set "QA_STATE=%~dp0..\..\target-windows\winforms-test-state\preview-%RANDOM%-%RANDOM%"
set "SPLINED_INTERNAL_SETTINGS_TEST_DIR=%QA_STATE%"
set "SPLINED_LEGACY_SETTINGS_TEST_DIR=%QA_STATE%\legacy"
set "SPLINED_QA_OUTPUT=%QA_OUT%\replacement"
set "CSC=%WINDIR%\Microsoft.NET\Framework64\v4.0.30319\csc.exe"
if not exist "%CSC%" set "CSC=%WINDIR%\Microsoft.NET\Framework\v4.0.30319\csc.exe"
if not exist "%CSC%" (
  popd
  exit /b 1
)
if not exist "%QA_OUT%" mkdir "%QA_OUT%"
if not exist "%QA_STATE%" mkdir "%QA_STATE%"
if not exist "%SPLINED_QA_OUTPUT%" mkdir "%SPLINED_QA_OUTPUT%"
"%CSC%" /nologo /target:winexe /platform:x64 /main:Splined.WindowsGui.ReplacementPreviewQaHarness /optimize+ /win32icon:app.ico /resource:splined-watermark.png,Splined.WindowsGui.Resources.splined-watermark.png /resource:splined-app-icon.png,Splined.WindowsGui.Resources.splined-app-icon.png /out:"%QA_OUT%\preview-qa.exe" /reference:System.dll /reference:System.Core.dll /reference:System.Drawing.dll /reference:System.Windows.Forms.dll /reference:System.Web.Extensions.dll ReleaseInfo.cs BuildInfo.Test.cs Program.cs AppIcon.cs EmbeddedAssets.cs NativeCore.cs ConfigState.cs BackupWindows.cs MusicBrainzMatchesForm.cs RuntimeLog.cs UpdateService.cs ThemeManager.cs SetupForm.cs SupportWindows.cs LibraryModel.cs ArtworkWorkspaceModel.cs ArtworkWorkspaceControls.cs NavidromeClient.cs MainForm.cs MainForm.ReplacementShell.cs ReplacementPreviewQaHarness.cs
if errorlevel 1 (
  popd
  exit /b 1
)
"%QA_OUT%\preview-qa.exe"
set "QA_EXIT=%ERRORLEVEL%"
popd
exit /b %QA_EXIT%
