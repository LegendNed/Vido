# Downloads the FFmpeg shared dev build (headers + import libs + DLLs) that ClipDeck links against.
# Source: BtbN/FFmpeg-Builds (https://github.com/BtbN/FFmpeg-Builds). Verified against the release checksums.
#
# Usage: pwsh scripts/fetch-ffmpeg.ps1 [-Version 8.1] [-Variant gpl|lgpl]
param(
    [string]$Version = "8.1",
    [ValidateSet("gpl", "lgpl")][string]$Variant = "gpl"
)
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$root = Split-Path -Parent $PSScriptRoot
$dest = Join-Path $root "third_party\ffmpeg"
$name = "ffmpeg-n$Version-latest-win64-$Variant-shared-$Version"
$base = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest"

$tmp = Join-Path ([IO.Path]::GetTempPath()) ("clipdeck-ffmpeg-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    $zip = Join-Path $tmp "$name.zip"
    $sums = Join-Path $tmp "checksums.sha256"
    Write-Host "Downloading $name.zip ..."
    Invoke-WebRequest -Uri "$base/$name.zip" -OutFile $zip
    Invoke-WebRequest -Uri "$base/checksums.sha256" -OutFile $sums

    $line = Get-Content $sums | Where-Object { $_ -match [regex]::Escape("$name.zip") } | Select-Object -First 1
    if (-not $line) { throw "No checksum entry for $name.zip" }
    $expected = ($line -split "\s+")[0].ToLowerInvariant()
    $actual = (Get-FileHash -Path $zip -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($expected -ne $actual) { throw "Checksum mismatch for $name.zip (expected $expected, got $actual)" }
    Write-Host "Checksum OK ($actual)"

    $extract = Join-Path $tmp "x"
    Expand-Archive -Path $zip -DestinationPath $extract
    $inner = Join-Path $extract $name
    foreach ($sub in "bin", "include", "lib") {
        if (-not (Test-Path (Join-Path $inner $sub))) { throw "Archive is missing '$sub'" }
    }
    if (Test-Path $dest) { Remove-Item -Recurse -Force $dest }
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $dest) | Out-Null
    Move-Item -Path $inner -Destination $dest
    Set-Content -Path (Join-Path $dest "VERSION.txt") -Value "$name`nsha256=$actual"
    Write-Host "FFmpeg installed to $dest"
}
finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
