@echo off
setlocal
powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "$ErrorActionPreference = 'Stop'; try { Invoke-Expression (Invoke-RestMethod -Uri 'https://github.com/Tiago-0liveira/Featherspot/releases/latest/download/install.ps1') } catch { Write-Error $_; exit 1 }"
exit /b %ERRORLEVEL%
