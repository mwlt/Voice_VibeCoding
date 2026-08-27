@echo off
cd /d "%~dp0"
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0install-winuhid-ui-force.ps1"
set RC=%ERRORLEVEL%
echo.
pause
exit /b %RC%
