@echo off
cd /d "%~dp0"
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0install-t1-hid-filter.ps1" -Mode Status
echo.
pause
exit /b %ERRORLEVEL%
