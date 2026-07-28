# Sync src/glasses-web/dist into APK assets so glasses WebView ships UI
# without relying on Desktop-hosted mobile-web uploads.
#
# Usage:
#   pwsh src/apps/mobile/android/tools/sync-glasses-web-assets.ps1
# Optional rebuild:
#   pwsh src/apps/mobile/android/tools/sync-glasses-web-assets.ps1 -Build

param(
    [switch]$Build
)

$ErrorActionPreference = "Stop"
$androidRoot = Split-Path -Parent $PSScriptRoot
$repoRoot = (Resolve-Path (Join-Path $androidRoot "..\..\..\..")).Path
$dist = Join-Path $repoRoot "src\glasses-web\dist"
$assets = Join-Path $androidRoot "app\src\main\assets\glasses-web"

if ($Build) {
    Write-Host "Building glasses-web..."
    pnpm --dir (Join-Path $repoRoot "src\glasses-web") run build
}

if (-not (Test-Path (Join-Path $dist "index.html"))) {
    throw "Missing $dist\index.html. Run with -Build or: pnpm --dir src/glasses-web run build"
}

if (Test-Path $assets) {
    Remove-Item -Recurse -Force $assets
}
# Remove legacy mobile-web asset bundle if present.
$legacy = Join-Path $androidRoot "app\src\main\assets\mobile-web"
if (Test-Path $legacy) {
    Remove-Item -Recurse -Force $legacy
}

New-Item -ItemType Directory -Path $assets | Out-Null
Copy-Item -Recurse -Force (Join-Path $dist "*") $assets
Write-Host "Synced glasses-web assets -> $assets"
Get-ChildItem -Recurse $assets | ForEach-Object { Write-Host ("  " + $_.FullName.Substring($assets.Length + 1)) }
