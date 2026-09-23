@echo off
rem codexSwitch for Windows without the exe. Keep this file and
rem codexSwitch-script.ps1 together in a folder on PATH.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0codexSwitch-script.ps1" %*
