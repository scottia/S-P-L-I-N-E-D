@echo off
setlocal
pushd "%~dp0.."
set "SPLINED_BUILD_GUI=1"
cargo build --locked --release --target x86_64-pc-windows-msvc
set "EXIT_CODE=%ERRORLEVEL%"
popd
exit /b %EXIT_CODE%
