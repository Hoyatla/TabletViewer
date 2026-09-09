#!/usr/bin/env pwsh
# Download the vision model weights into models/vision/<id>/model.onnx
# based on the URL declared in each modele.json.
#
# Usage:  pwsh models/download.ps1
#         pwsh models/download.ps1 -Only yolox-nano
#
# Idempotent: skips files that already exist (size > 1 KB). Use
# `-Force` to re-download.

[CmdletBinding()]
param(
    [string[]]$Only = @(),
    [switch]$Force
)

$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
$visionDir = Join-Path $root 'vision'

if (-not (Test-Path $visionDir)) {
    Write-Error "Dossier $visionDir introuvable. Lance depuis la racine du repo pc-agent."
}

$manifests = Get-ChildItem -Path $visionDir -Recurse -Filter 'modele.json'
foreach ($m in $manifests) {
    $id = $m.Directory.Name
    if ($Only.Count -gt 0 -and $Only -notcontains $id) { continue }

    $json = Get-Content -Raw -Path $m.FullName | ConvertFrom-Json
    $url = $json.url_poids
    if (-not $url) {
        Write-Warning "[$id] pas de url_poids dans $($m.FullName)"
        continue
    }
    $target = Join-Path $m.DirectoryName $json.fichiers.poids
    if ((Test-Path $target) -and (Get-Item $target).Length -gt 1024 -and -not $Force) {
        Write-Host "[$id] deja telecharge ($($(Get-Item $target).Length) octets) — skip"
        continue
    }
    Write-Host "[$id] telechargement $url"
    Write-Host "       -> $target"
    Invoke-WebRequest -Uri $url -OutFile $target -UseBasicParsing
    $size = (Get-Item $target).Length
    if ($size -lt 1024) {
        Write-Error "[$id] telechargement semble vide ($size octets) — URL valide ?"
    }
    Write-Host "[$id] OK ($size octets)"
}

Write-Host "Termine."
