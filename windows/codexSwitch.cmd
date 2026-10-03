@echo off
setlocal
rem Keep this file with codexSwitch-script.ps1 in a folder on PATH.
rem The script returns 10 only after confirming uninstall. Delete this launcher
rem with cmd's builtin on its already-read line, after PowerShell has returned.
set "CODEX_SWITCH_UNINSTALL_LAUNCHER="
if /i "%~1"=="uninstall" set "CODEX_SWITCH_UNINSTALL_LAUNCHER=%~f0"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0codexSwitch-script.ps1" %* && exit /b 0 || if errorlevel 11 (exit /b 1) else if errorlevel 10 (if /i "%~1"=="uninstall" ((goto) 2>nul & del /f "%~f0") else exit /b 1) else exit /b 1
