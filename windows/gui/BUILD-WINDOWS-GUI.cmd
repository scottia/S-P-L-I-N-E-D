@echo off
setlocal
set "CSC=%WINDIR%\Microsoft.NET\Framework64\v4.0.30319\csc.exe"
if not exist "%CSC%" set "CSC=%WINDIR%\Microsoft.NET\Framework\v4.0.30319\csc.exe"
if not exist "%CSC%" (
  echo Microsoft .NET Framework C# compiler was not found.
  exit /b 1
)
if not exist "qa-test" mkdir "qa-test"
"%CSC%" /nologo /target:exe /out:qa-test\icon-generator.exe /reference:System.dll /reference:System.Drawing.dll IconGenerator.cs
if errorlevel 1 exit /b %errorlevel%
qa-test\icon-generator.exe splined-watermark.png app.ico splined-app-icon.png
if errorlevel 1 exit /b %errorlevel%
"%CSC%" /nologo /target:winexe /main:Splined.WindowsGui.Program /optimize+ /win32manifest:app.manifest /win32icon:app.ico /out:SPLINED-GUI.exe /reference:System.dll /reference:System.Core.dll /reference:System.Drawing.dll /reference:System.Windows.Forms.dll /reference:System.Web.Extensions.dll ReleaseInfo.cs Program.cs AssemblyInfo.cs AppIcon.cs ConfigState.cs ThemeManager.cs SetupForm.cs SupportWindows.cs LibraryModel.cs MainForm.cs
exit /b %errorlevel%
