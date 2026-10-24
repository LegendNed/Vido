# Builds the Vido trailer (trailer/Vido-trailer.mp4): stand-in gameplay frames,
# music, sound effects and voice-over, then renders trailer.html frame by frame
# and mixes everything together. Intermediate files go to trailer/build/.
#
# Needs: Node, Python 3 with Pillow, Microsoft Edge, FFmpeg in third_party
# (pwsh scripts/fetch-ffmpeg.ps1) and, once, the voice model (-Setup).
#
# Usage: pwsh trailer/build.ps1 [-Setup] [-Capture] [-Preview] [-Out <file.mp4>]
#   -Setup    creates trailer/.tts (Python venv with kokoro-onnx) and downloads the
#             Kokoro model (~350 MB). Needed once, for the voice-over.
#   -Capture  re-takes the UI screenshots in trailer/shots from the real UI
#             (trailer/mock.html on Vite) - do this after UI changes.
#   -Preview  renders a contact sheet of stills (build/preview.png) instead of the video.
param([switch]$Setup, [switch]$Capture, [switch]$Preview, [string]$Out = "")
$ErrorActionPreference = "Stop"

$here = $PSScriptRoot
$root = Split-Path -Parent $here
$build = Join-Path $here "build"
$tts = Join-Path $here ".tts"
$ffmpeg = Join-Path $root "third_party\ffmpeg\bin\ffmpeg.exe"
if (-not $Out) { $Out = Join-Path $here "Vido-trailer.mp4" }
if (-not (Test-Path $ffmpeg)) { throw "Missing $ffmpeg - run: pwsh scripts/fetch-ffmpeg.ps1" }
New-Item -ItemType Directory -Force (Join-Path $build "audio") | Out-Null

function Run([string]$exe, [string[]]$arguments) {
    & $exe @arguments
    if ($LASTEXITCODE -ne 0) { throw "$exe $($arguments -join ' ') failed" }
}

if ($Setup) {
    Write-Host "Setting up the voice model in $tts..."
    if (-not (Test-Path "$tts\venv")) { Run python @("-m", "venv", "$tts\venv") }
    Run "$tts\venv\Scripts\python.exe" @("-m", "pip", "install", "--quiet", "--disable-pip-version-check", "kokoro-onnx", "soundfile")
    New-Item -ItemType Directory -Force "$tts\model" | Out-Null
    $release = "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0"
    foreach ($f in "kokoro-v1.0.onnx", "voices-v1.0.bin") {
        if (-not (Test-Path "$tts\model\$f")) {
            Write-Host "Downloading $f..."
            Invoke-WebRequest "$release/$f" -OutFile "$tts\model\$f"
        }
    }
}

Write-Host "Stand-in gameplay frames..."
Run python @("$here\make_frames.py", "$build\assets", "$build\mock-assets")

if ($Capture) {
    Write-Host "Capturing the UI..."
    # The editor needs a real video to load (the frame captured over it is f0.png).
    Run $ffmpeg @("-hide_banner", "-loglevel", "error", "-y", "-loop", "1", "-framerate", "30", "-i", "$build\assets\g0.png",
        "-t", "30", "-vf", "scale=2560:1440,format=yuv420p", "-c:v", "libx264", "-preset", "veryfast", "-crf", "26", "$build\mock-assets\clip.mp4")
    $vite = Start-Process -FilePath "pnpm" -ArgumentList "exec", "vite", "--port", "5199", "--strictPort" -WorkingDirectory $root -PassThru -WindowStyle Hidden
    try {
        Start-Sleep -Seconds 6
        Run node @("$here\capture.mjs", "$here\shots", "$here\capture-steps.json")
    } finally {
        # Vite runs under pnpm/node: stop whatever listens on the port.
        Get-NetTCPConnection -LocalPort 5199 -State Listen -ErrorAction SilentlyContinue |
            ForEach-Object { Stop-Process -Id $_.OwningProcess -Force -ErrorAction SilentlyContinue }
        Stop-Process -Id $vite.Id -Force -ErrorAction SilentlyContinue
    }
}

if ($Preview) {
    Run node @("$here\render.mjs", "stills", "$build\stills", "1.5,7.6,12,16,20.8,23.5,29.5,36,42,48,53,58")
    Run python @("$here\sheet.py", "$build\stills", "$build\preview.png")
    Write-Host "Preview: $build\preview.png"
    exit 0
}

if (-not (Test-Path "$tts\venv\Scripts\python.exe") -or -not (Test-Path "$tts\model\kokoro-v1.0.onnx")) {
    throw "The voice model is missing - run once: pwsh trailer/build.ps1 -Setup"
}

Write-Host "Music, sound effects and voice-over..."
Run python @("$here\music.py", "$build\audio\music.wav", "lofi", "60")
Run python @("$here\sfx.py", "$build\audio")
Run "$tts\venv\Scripts\python.exe" @("$here\voice.py", "$tts\model", "$build\audio\voice")

Write-Host "Rendering the video (a few minutes)..."
Run node @("$here\render.mjs", "video", "$build\video.mp4", "30")

Write-Host "Mixing..."
Run python @("$here\mix.py", $build, $Out)
Write-Host "Done: $Out"
