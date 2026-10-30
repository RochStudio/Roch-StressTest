@echo off
setlocal
cd /d "%~dp0"

REM Builds the EXE the first time, then starts it. RochStressTest.exe raises
REM its own UAC prompt, so there is no elevation to do here.
REM
REM When something needs explaining, run a debug build from a prompt instead
REM -- it keeps a console for its diagnostics:
REM
REM     cargo run -- --no-elevate
if not exist "%~dp0RochStressTest.exe" (
    call "%~dp0BUILD_EXE.bat" nopause
    if errorlevel 1 (
        echo Build failed. Rust ^(cargo^) is required to build from source.
        pause
        exit /b 1
    )
)
start "" "%~dp0RochStressTest.exe"
exit /b 0
