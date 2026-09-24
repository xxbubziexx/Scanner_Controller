@echo off
title Build Scanner Controller Portable EXE
echo ===================================================
echo  Building Scanner Controller Standalone Portable EXE
echo ===================================================
echo.

python -m pip install pyinstaller
if %errorlevel% neq 0 (
    echo [ERROR] Failed to verify or install PyInstaller.
    pause
    exit /b %errorlevel%
)

echo.
echo Running PyInstaller...
pyinstaller --clean --noconfirm scanner_controller.spec

if %errorlevel% neq 0 (
    echo.
    echo [ERROR] Build failed! Check the output above.
    pause
    exit /b %errorlevel%
)

echo.
echo ===================================================
echo  Build Succeeded!
echo  Portable Executable: dist\ScannerController.exe
echo ===================================================
echo.
pause
