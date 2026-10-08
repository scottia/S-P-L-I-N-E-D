@echo off
setlocal
pushd "%~dp0"
set "QA_ROOT=%~dp0..\..\target-windows\winforms-qa"
set "QA_OUT=%QA_ROOT%\winforms"
set "QA_STATE=%~dp0..\..\target-windows\winforms-test-state"
set "SPLINED_INTERNAL_SETTINGS_TEST_DIR=%QA_STATE%\state-%RANDOM%-%RANDOM%"
set "SPLINED_LEGACY_SETTINGS_TEST_DIR=%QA_STATE%\legacy-%RANDOM%-%RANDOM%"
set "SPLINED_GUI_SOURCE_ROOT=%~dp0"
set "CSC=%WINDIR%\Microsoft.NET\Framework64\v4.0.30319\csc.exe"
if not exist "%CSC%" set "CSC=%WINDIR%\Microsoft.NET\Framework\v4.0.30319\csc.exe"
if not exist "%CSC%" (
  echo Microsoft .NET Framework C# compiler was not found.
  popd
  exit /b 1
)
if not exist "%QA_OUT%" mkdir "%QA_OUT%"
if not exist "%QA_STATE%" mkdir "%QA_STATE%"
"%CSC%" /nologo /target:exe /platform:x64 /main:Splined.WindowsGui.GuiSelfTests /optimize+ /win32icon:app.ico /resource:splined-watermark.png,Splined.WindowsGui.Resources.splined-watermark.png /resource:splined-app-icon.png,Splined.WindowsGui.Resources.splined-app-icon.png /out:"%QA_OUT%\splined-winforms-tests.exe" /reference:System.dll /reference:System.Core.dll /reference:System.Drawing.dll /reference:System.Windows.Forms.dll /reference:System.Web.Extensions.dll ReleaseInfo.cs BuildInfo.Test.cs Program.cs AppIcon.cs EmbeddedAssets.cs NativeCore.cs ConfigState.cs BackupWindows.cs MusicBrainzMatchesForm.cs RuntimeLog.cs UpdateService.cs ThemeManager.cs SetupForm.cs SupportWindows.cs LibraryModel.cs MainForm.cs GuiSelfTests.cs
if errorlevel 1 (
  popd
  exit /b 1
)
"%QA_OUT%\splined-winforms-tests.exe"
set "QA_EXIT=%ERRORLEVEL%"
popd
exit /b %QA_EXIT%
