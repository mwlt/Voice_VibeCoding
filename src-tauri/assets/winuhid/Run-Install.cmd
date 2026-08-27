@echo off
cd /d "%~dp0"
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0install-winuhid-ui.ps1"
set RC=%ERRORLEVEL%
echo.
pause
exit /b %RC%
