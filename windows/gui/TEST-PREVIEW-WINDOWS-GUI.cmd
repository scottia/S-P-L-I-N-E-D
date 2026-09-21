@echo off
setlocal
set "CSC=%WINDIR%\Microsoft.NET\Framework64\v4.0.30319\csc.exe"
if not exist "%CSC%" set "CSC=%WINDIR%\Microsoft.NET\Framework\v4.0.30319\csc.exe"
if not exist "%CSC%" exit /b 1
if not exist "qa-test" mkdir "qa-test"
"%CSC%" /nologo /target:winexe /main:Splined.WindowsGui.PreviewQaHarness /optimize+ /win32icon:app.ico /resource:splined-watermark.png,Splined.WindowsGui.Resources.splined-watermark.png /resource:splined-app-icon.png,Splined.WindowsGui.Resources.splined-app-icon.png /out:qa-test\preview-qa.exe /reference:System.dll /reference:System.Core.dll /reference:System.Drawing.dll /reference:System.Windows.Forms.dll /reference:System.Web.Extensions.dll ReleaseInfo.cs Program.cs AppIcon.cs EmbeddedAssets.cs ConfigState.cs ThemeManager.cs SetupForm.cs SupportWindows.cs LibraryModel.cs MainForm.cs PreviewQaHarness.cs
exit /b %errorlevel%
