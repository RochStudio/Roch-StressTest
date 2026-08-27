@echo off
setlocal
cd /d "%~dp0"

py -V:3.13 -m pip install -r requirements.txt
if errorlevel 1 (
    echo Dependency installation failed. Python 3.13 x64 is required.
    pause
    exit /b 1
)

REM pyw rather than py: py is the console launcher, so Windows gives it a
REM black console window that sits behind the app for the whole session.
REM main.py raises its own UAC prompt, so there is no elevation to do here.
REM
REM Diagnostics are what this costs. pyw discards stdout and stderr, so a
REM startup traceback goes nowhere. When something needs explaining, run it
REM the loud way from a prompt:
REM
REM     py -V:3.13 main.py
start "" pyw -V:3.13 main.py
exit /b 0
