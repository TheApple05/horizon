@echo off
cd /d "%~dp0"
horizon-offline.exe
if errorlevel 1 pause
