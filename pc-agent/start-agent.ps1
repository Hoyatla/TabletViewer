# Launches the pc-agent in the foreground with the same defaults the
# ScreenViewerOnTablet Android app expects. Bind on 0.0.0.0 so the
# tablet can reach it over the LAN; no token (dev session on a
# trusted LAN).
#
# Run from this directory:
#   powershell -ExecutionPolicy Bypass -File .\start-agent.ps1
#
# Or just double-click this file in Explorer (PowerShell will open
# a console window and run the agent there; close it to stop the
# agent).

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$bin = Join-Path $root 'target\debug\pc-agent.exe'

if (-not (Test-Path -LiteralPath $bin)) {
    Write-Output "Agent binary not found at $bin"
    Write-Output "Build it first:  cd $root ; cargo build"
    exit 1
}

$env:RUST_LOG = 'pc_agent=info,mdns_sd=warn'
Write-Output "Launching $bin"
Write-Output "  port:  8765  (override with --port or PC_AGENT_PORT)"
Write-Output "  bind:  0.0.0.0  (override with --bind or PC_AGENT_BIND)"
Write-Output "  token: none    (set --token or PC_AGENT_TOKEN to require auth)"
Write-Output "  roots: none    (set --roots to restrict /v1/file and /v1/log)"
Write-Output "  Ctrl-C in this window, or close it, to stop the agent."
Write-Output ""

& $bin --port 8765 --bind 0.0.0.0
