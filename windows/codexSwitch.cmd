@echo off
rem codexSwitch for Windows without the exe. Keep this file and
rem codexSwitch-launch.ps1 together in a folder on PATH.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0codexSwitch-launch.ps1" %*
