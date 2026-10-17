@echo off
setlocal
rem Runs the Windows test wizard with the built-in Windows PowerShell, as a
rem per-process setting only (no execution-policy or registry change).
where powershell.exe >nul 2>nul
if errorlevel 1 (
  echo Windows PowerShell ^(powershell.exe^) was not found on PATH.
  set "wizard_exit=1"
  goto finish
)
powershell.exe -NoProfile -ExecutionPolicy Bypass -STA -File "%~dp0scripts\windows-test-wizard.ps1" %*
set "wizard_exit=%ERRORLEVEL%"
:finish
echo.
pause
exit /b %wizard_exit%
