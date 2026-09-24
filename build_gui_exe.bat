@echo off
title Build Scanner Controller Native GUI Portable EXE
echo =======================================================
echo  Building Scanner Controller Native Desktop GUI (EXE)
echo =======================================================
echo.

python -m pip install pyinstaller customtkinter darkdetect
if %errorlevel% neq 0 (
    echo [ERROR] Failed to verify or install required packages.
    pause
    exit /b %errorlevel%
)

echo.
echo Running PyInstaller for Native GUI...
pyinstaller --clean --noconfirm scanner_controller_gui.spec

if %errorlevel% neq 0 (
    echo.
    echo [ERROR] Build failed! Check the output above.
    pause
    exit /b %errorlevel%
)

echo.
echo =======================================================
echo  Build Succeeded!
echo  Portable GUI: dist\ScannerControllerGUI.exe
echo =======================================================
echo.
pause
