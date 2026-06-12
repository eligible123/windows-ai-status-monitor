$ErrorActionPreference = "Stop"
Set-Location (Split-Path -Parent $PSScriptRoot)
npm.cmd install
npm.cmd run package:exe
Write-Host "Installer output: src-tauri\target\release\bundle\nsis"
