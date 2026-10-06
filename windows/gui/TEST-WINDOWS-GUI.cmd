@echo off
setlocal
set "SPLINED_INTERNAL_SETTINGS_TEST_DIR=%TEMP%\splined-windows-gui-tests-%RANDOM%-%RANDOM%"
set "SPLINED_LEGACY_SETTINGS_TEST_DIR=%TEMP%\splined-windows-gui-legacy-%RANDOM%-%RANDOM%"
set "CSC=%WINDIR%\Microsoft.NET\Framework64\v4.0.30319\csc.exe"
if not exist "%CSC%" set "CSC=%WINDIR%\Microsoft.NET\Framework\v4.0.30319\csc.exe"
if not exist "%CSC%" (
  echo Microsoft .NET Framework C# compiler was not found.
  exit /b 1
)
if not exist "qa-test" mkdir "qa-test"
"%CSC%" /nologo /target:exe /main:Splined.WindowsGui.GuiSelfTests /optimize+ /win32icon:app.ico /resource:splined-watermark.png,Splined.WindowsGui.Resources.splined-watermark.png /resource:splined-app-icon.png,Splined.WindowsGui.Resources.splined-app-icon.png /out:qa-test\gui-tests.exe /reference:System.dll /reference:System.Core.dll /reference:System.Drawing.dll /reference:System.Windows.Forms.dll /reference:System.Web.Extensions.dll ReleaseInfo.cs BuildInfo.Test.cs Program.cs AppIcon.cs EmbeddedAssets.cs ConfigState.cs BackupWindows.cs MusicBrainzMatchesForm.cs RuntimeLog.cs UpdateService.cs ThemeManager.cs SetupForm.cs SupportWindows.cs LibraryModel.cs MainForm.cs GuiSelfTests.cs
if errorlevel 1 exit /b %errorlevel%
qa-test\gui-tests.exe
exit /b %errorlevel%
