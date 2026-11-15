@echo off
setlocal
title Uniden Scanner Mutual Exclusion Controller (Rust)
set "PATH=%USERPROFILE%\.cargo\bin;C:\Users\MAC10\AppData\Local\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin;%PATH%"

if exist "%~dp0target\release\scanner-controller.exe" (
    echo Starting Uniden Scanner Controller [Release Binary]...
    "%~dp0target\release\scanner-controller.exe" %*
) else (
    echo Building and starting Uniden Scanner Controller [Cargo Release]...
    cargo run --release --bin scanner-controller -- %*
)

