@echo off
setlocal
cd /d "%~dp0"
set "NO_PAUSE=%~1"

echo Building Roch StressTest with Python 3.13 x64
echo.

py -V:3.13 --version
if errorlevel 1 (
    echo.
    echo Python 3.13 x64 was not found. Install it, then try again.
    if /i not "%NO_PAUSE%"=="nopause" pause
    exit /b 1
)

echo.
echo Installing required Python packages...
py -V:3.13 -m pip install -r requirements.txt
if errorlevel 1 goto fail

echo.
echo Cleaning old build folders...
rmdir /s /q "%~dp0build" 2>nul
rmdir /s /q "%~dp0dist" 2>nul

echo.
echo Building the GUI...
py -V:3.13 -m PyInstaller --clean -y RochStressTest.spec
if errorlevel 1 goto fail

if not exist "%~dp0dist\RochStressTest.exe" goto missing

echo.
echo Done. The EXE is here:
echo %~dp0dist\RochStressTest.exe
echo.
echo Copy it up one level, next to the tool folders, before running it --
echo it looks for Prime95, y-cruncher and the rest beside itself.
if /i not "%NO_PAUSE%"=="nopause" (
    start "" "%~dp0dist"
    pause
)
exit /b 0

:missing
echo.
echo PyInstaller finished, but RochStressTest.exe was not found.
goto fail

:fail
echo.
echo Build failed. Review the error output above.
if /i not "%NO_PAUSE%"=="nopause" pause
exit /b 1
