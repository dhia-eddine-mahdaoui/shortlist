# Builds Shortlist, installs it for this Windows user, and connects Claude Code / Codex.
# Usage:  powershell -ExecutionPolicy Bypass -File install.ps1
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

foreach ($tool in 'node', 'npm', 'cargo') {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        throw "$tool was not found. Install Node.js 20+ and Rust (https://rustup.rs) first."
    }
}

Write-Host 'Installing build tools...'
npm install --no-fund --no-audit | Out-Null

Write-Host 'Building Shortlist (the first build takes a few minutes)...'
npm run desktop
if ($LASTEXITCODE -ne 0) { throw 'Build failed.' }

$setup = Get-ChildItem 'src-tauri\target\release\bundle\nsis\*-setup.exe' | Sort-Object LastWriteTime | Select-Object -Last 1
Write-Host "Installing $($setup.Name)..."
Start-Process $setup.FullName -ArgumentList '/S' -Wait

$exe = Join-Path $env:LOCALAPPDATA 'Shortlist\shortlist.exe'
if (Test-Path $exe) { Start-Process $exe }

$url = 'http://127.0.0.1:12731/mcp'
if (Get-Command claude -ErrorAction SilentlyContinue) {
    claude mcp add --transport http --scope user shortlist $url 2>$null
    Write-Host 'Connected to Claude Code.'
}
$codex = Join-Path $env:USERPROFILE '.codex\config.toml'
if ((Get-Command codex -ErrorAction SilentlyContinue) -or (Test-Path $codex)) {
    New-Item -ItemType Directory -Force (Split-Path $codex) | Out-Null
    if (-not (Test-Path $codex) -or -not (Select-String -Path $codex -Pattern '^\[mcp_servers\.shortlist\]' -Quiet)) {
        Add-Content -Path $codex -Encoding utf8 -Value "`n[mcp_servers.shortlist]`nurl = `"$url`""
        Write-Host 'Connected to Codex.'
    }
}

$ext = Join-Path $env:APPDATA 'Shortlist\chrome-extension'
Write-Host ''
Write-Host 'Last step, in Chrome:'
Write-Host '  1. Open chrome://extensions and turn on Developer mode.'
Write-Host "  2. Load unpacked -> $ext"
Write-Host '  3. Stay signed in to LinkedIn.'
