@echo off
setlocal
set "CSC=%WINDIR%\Microsoft.NET\Framework64\v4.0.30319\csc.exe"
if not exist "%CSC%" set "CSC=%WINDIR%\Microsoft.NET\Framework\v4.0.30319\csc.exe"
if not exist "%CSC%" (
  echo Microsoft .NET Framework C# compiler was not found.
  exit /b 1
)
if not exist "qa-test" mkdir "qa-test"
copy /Y "splined-watermark.png" "qa-test\splined-watermark.png" >nul
if errorlevel 1 exit /b %errorlevel%
"%CSC%" /nologo /target:exe /out:qa-test\icon-generator.exe /reference:System.dll /reference:System.Drawing.dll IconGenerator.cs
if errorlevel 1 exit /b %errorlevel%
qa-test\icon-generator.exe splined-watermark.png qa-test\app.ico qa-test\splined-app-icon.png
if errorlevel 1 exit /b %errorlevel%
"%CSC%" /nologo /target:exe /main:Splined.WindowsGui.GuiSelfTests /optimize+ /win32icon:qa-test\app.ico /out:qa-test\gui-tests.exe /reference:System.dll /reference:System.Core.dll /reference:System.Drawing.dll /reference:System.Windows.Forms.dll /reference:System.Web.Extensions.dll ReleaseInfo.cs Program.cs AppIcon.cs ConfigState.cs ThemeManager.cs SetupForm.cs SupportWindows.cs LibraryModel.cs MainForm.cs GuiSelfTests.cs
if errorlevel 1 exit /b %errorlevel%
qa-test\gui-tests.exe
exit /b %errorlevel%
