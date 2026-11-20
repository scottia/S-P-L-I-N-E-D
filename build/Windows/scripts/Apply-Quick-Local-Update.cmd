@echo off
setlocal
title SPLINED Quick Local Update
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0Install-LocalTestBuild.ps1" -Quick
set "SPLINED_BUILD_EXIT=%ERRORLEVEL%"
echo.
if not "%SPLINED_BUILD_EXIT%"=="0" echo Quick local update failed. Review the message above.
pause
exit /b %SPLINED_BUILD_EXIT%
