@echo off
setlocal
set "CSC=%WINDIR%\Microsoft.NET\Framework64\v4.0.30319\csc.exe"
if not exist "%CSC%" set "CSC=%WINDIR%\Microsoft.NET\Framework\v4.0.30319\csc.exe"
if not exist "%CSC%" (
  echo Microsoft .NET Framework C# compiler was not found.
  exit /b 1
)
if not exist "qa-test" mkdir "qa-test"
"%CSC%" /nologo /target:exe /main:Splined.WindowsGui.CredentialQaHarness /optimize+ /win32icon:app.ico /resource:splined-watermark.png,Splined.WindowsGui.Resources.splined-watermark.png /resource:splined-app-icon.png,Splined.WindowsGui.Resources.splined-app-icon.png /out:qa-test\credential-qa.exe /reference:System.dll /reference:System.Core.dll /reference:System.Drawing.dll /reference:System.Windows.Forms.dll /reference:System.Web.Extensions.dll ReleaseInfo.cs Program.cs AppIcon.cs EmbeddedAssets.cs ConfigState.cs ThemeManager.cs SetupForm.cs SupportWindows.cs LibraryModel.cs MainForm.cs GuiSelfTests.cs CredentialQaHarness.cs
if errorlevel 1 exit /b %errorlevel%
qa-test\credential-qa.exe
exit /b %errorlevel%
