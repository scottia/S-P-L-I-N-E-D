@echo off
setlocal
pushd "%~dp0.."
cargo build --locked --release
set "EXIT_CODE=%ERRORLEVEL%"
popd
exit /b %EXIT_CODE%
