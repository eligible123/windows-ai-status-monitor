$ErrorActionPreference = "Stop"
Set-Location (Split-Path -Parent $PSScriptRoot)
npm.cmd install
npm.cmd run tauri:dev
