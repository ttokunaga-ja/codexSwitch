@echo off
rem codexSwitch for Windows without the exe. Keep this file and
rem codexSwitch-script.ps1 together in a folder on PATH.
rem The exit is on the same line because cmd reads a running .cmd as it
rem goes, and codexSwitch update may replace this file.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0codexSwitch-script.ps1" %* && exit /b 0 || exit /b 1
