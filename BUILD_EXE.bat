@echo off
setlocal
cd /d "%~dp0"
set "NO_PAUSE=%~1"

echo Building Roch StressTest with Rust (cargo)
echo.

where cargo >nul 2>nul
if errorlevel 1 (
    echo.
    echo cargo was not found. Install Rust from https://rustup.rs ^(the
    echo x86_64-pc-windows-msvc toolchain, with the Visual Studio C++ build
    echo tools it asks for^), then try again.
    if /i not "%NO_PAUSE%"=="nopause" pause
    exit /b 1
)

cargo --version
echo.
echo Building the release EXE...
cargo build --release
if errorlevel 1 goto fail

if not exist "%~dp0target\release\RochStressTest.exe" goto missing

REM The program looks for Prime95, y-cruncher and the rest beside itself, so
REM the EXE is copied up next to the tool folders rather than left in target.
copy /y "%~dp0target\release\RochStressTest.exe" "%~dp0RochStressTest.exe" >nul
if errorlevel 1 goto fail

echo.
echo Done. The EXE is here, beside the tool folders:
echo %~dp0RochStressTest.exe
if /i not "%NO_PAUSE%"=="nopause" pause
exit /b 0

:missing
echo.
echo cargo finished, but target\release\RochStressTest.exe was not found.
goto fail

:fail
echo.
echo Build failed. Review the error output above.
if /i not "%NO_PAUSE%"=="nopause" pause
exit /b 1
