# Builds a portable Vido folder + zip:
#   release/Vido-<version>-portable-x64/
#     vido.exe, FFmpeg DLLs, portable (marker), LICENSE, THIRD_PARTY_NOTICES.md, licenses/
# With the `portable` marker present, settings, logs, cache and the WebView2 profile
# are kept in .\data next to the exe. Clips still default to Videos\Vido
# (change the output folder in Settings → Saving to keep everything on a USB drive).
#
# Usage: pwsh scripts/package-portable.ps1 [-SkipBuild]
param([switch]$SkipBuild)
$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$version = (Get-Content (Join-Path $root "package.json") -Raw | ConvertFrom-Json).version

if (-not $SkipBuild) {
    Write-Host "Building release executable..."
    pnpm tauri build --no-bundle
    if ($LASTEXITCODE -ne 0) { throw "tauri build failed" }
}

$exe = Join-Path $root "src-tauri\target\release\vido.exe"
if (-not (Test-Path $exe)) { throw "Missing $exe — run without -SkipBuild" }

$name = "Vido-$version-portable-x64"
$outDir = Join-Path $root "release\$name"

# Someone may have run the previous portable build: keep its settings/logs (.\data)
# out of the zip, but don't destroy them.
$userData = Join-Path $outDir "data"
$stash = $null
if (Test-Path $userData) {
    if (Get-Process -Name vido -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$outDir*" }) {
        throw "Vido is running from $outDir — quit it (tray → Quit) and run this script again."
    }
    $stash = Join-Path $root "release\.$name-data-$(Get-Date -Format yyyyMMddHHmmss)"
    Move-Item -LiteralPath $userData -Destination $stash
}

if (Test-Path $outDir) { Remove-Item -LiteralPath $outDir -Recurse -Force }
New-Item -ItemType Directory -Force (Join-Path $outDir "licenses") | Out-Null

Copy-Item $exe $outDir
Get-ChildItem (Join-Path $root "third_party\ffmpeg\bin") -Filter *.dll |
    Where-Object { $_.Name -notlike "avdevice*" } |
    ForEach-Object { Copy-Item $_.FullName $outDir }
Copy-Item (Join-Path $root "LICENSE") $outDir
Copy-Item (Join-Path $root "THIRD_PARTY_NOTICES.md") $outDir
Copy-Item (Join-Path $root "third_party\ffmpeg\LICENSE.txt") (Join-Path $outDir "licenses\FFmpeg-LICENSE.txt")
Set-Content -Path (Join-Path $outDir "portable") -Value "This file enables portable mode: data is stored in .\data next to vido.exe."

$zip = Join-Path $root "release\$name.zip"
if (Test-Path $zip) { Remove-Item -LiteralPath $zip -Force }
Compress-Archive -Path (Join-Path $outDir "*") -DestinationPath $zip -CompressionLevel Optimal

if ($stash) {
    Move-Item -LiteralPath $stash -Destination $userData
    Write-Host "Kept existing portable data in $userData (not included in the zip)."
}

$size = [math]::Round((Get-Item $zip).Length / 1MB, 1)
Write-Host "Portable build: $outDir"
Write-Host "Zip: $zip ($size MB)"
Write-Host "Note: requires the Microsoft Edge WebView2 Runtime (preinstalled on Windows 11)."
