@echo off
setlocal
pushd "%~dp0"
set "QA_ROOT=%~dp0..\..\target-windows\winforms-qa"
set "QA_OUT=%QA_ROOT%\preview"
set "CSC=%WINDIR%\Microsoft.NET\Framework64\v4.0.30319\csc.exe"
if not exist "%CSC%" set "CSC=%WINDIR%\Microsoft.NET\Framework\v4.0.30319\csc.exe"
if not exist "%CSC%" (
  popd
  exit /b 1
)
if not exist "%QA_OUT%" mkdir "%QA_OUT%"
"%CSC%" /nologo /target:winexe /platform:x64 /main:Splined.WindowsGui.PreviewQaHarness /optimize+ /win32icon:app.ico /resource:splined-watermark.png,Splined.WindowsGui.Resources.splined-watermark.png /resource:splined-app-icon.png,Splined.WindowsGui.Resources.splined-app-icon.png /out:"%QA_OUT%\preview-qa.exe" /reference:System.dll /reference:System.Core.dll /reference:System.Drawing.dll /reference:System.Windows.Forms.dll /reference:System.Web.Extensions.dll ReleaseInfo.cs BuildInfo.Test.cs Program.cs AppIcon.cs EmbeddedAssets.cs NativeCore.cs ConfigState.cs BackupWindows.cs MusicBrainzMatchesForm.cs RuntimeLog.cs UpdateService.cs ThemeManager.cs SetupForm.cs SupportWindows.cs LibraryModel.cs MainForm.cs PreviewQaHarness.cs
set "QA_EXIT=%ERRORLEVEL%"
popd
exit /b %QA_EXIT%
