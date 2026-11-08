@echo off
for %%A in (%*) do (
    if "%%A"=="--version" (
        echo 0.29.2
        exit /b 0
    )
    if "%%A"=="--modversion" (
        echo 1.2.8
        exit /b 0
    )
)
echo -lasound
exit /b 0
