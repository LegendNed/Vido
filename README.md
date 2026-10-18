# ClipDeck

A lightweight, **local-first** replay-buffer clipper for Windows — like Medal.tv, minus the account,
cloud, ads, watermark and telemetry. ClipDeck keeps the last moments of your screen and audio in a
rolling buffer; press a hotkey and the last 15/30/60 s are saved as an MP4 in milliseconds. A built-in
editor trims, crops, mutes tracks and compresses clips to a target size.

- **Private by design:** no accounts, uploads, telemetry, ads, watermarks or auto-updates. Nothing leaves your PC.
- **Low overhead:** Windows.Graphics.Capture → GPU colour conversion → NVENC/AMF with **zero CPU copies**.
- **Instant saves:** the buffer stores _encoded_ packets, so saving is a remux (no re-encode).
- **Tauri v2** app: React + TypeScript UI, all capture/audio/encoding/OS work in Rust.

> Status: v0.1. Verified on Windows 11 with an NVIDIA RTX 5090 (NVENC H.264/HEVC/AV1). AMD AMF and
> Intel Quick Sync paths are implemented through FFmpeg but untested on real AMD/Intel hardware.

---

## Contents

1. [Features](#features)
2. [Building](#building)
3. [Installer & portable builds](#installer--portable-builds)
4. [First run](#first-run)
5. [How the replay buffer works](#how-the-replay-buffer-works)
6. [Hotkeys](#hotkeys)
7. [Windows APIs & permissions](#windows-apis--permissions)
8. [Settings & files](#settings--files)
9. [Known limitations](#known-limitations)
10. [Architecture](#architecture) (details in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md))
11. [License](#license)

---

## Features

**Capture**

- Display, window (re-found by program name after restarts) or rectangular region.
- 24/30/48/60/120 fps or native refresh rate (59.94 handled exactly, capped at 144).
- Source resolution or 1440p/1080p/720p downscale (never upscales), cursor on/off.
- Buffer 5 s – 10 min, in RAM or disk segments ("Auto" picks), optional 1–20 GB size cap.
- Exclusive-fullscreen detection: warns when the captured image stays black.
- HDR displays are detected and recorded as SDR (with a warning).

**Audio**

- Desktop audio (WASAPI loopback of any output device), microphone, up to 4 extra inputs.
- Per-track enable, mute, volume (0–200 %), gain (−20…+30 dB), mono downmix, device selection
  (or "follow Windows default").
- Optional RNNoise noise suppression for inputs (local, off by default).
- Each clip gets a **mix track** (what players play) plus **one track per source** for editing.
- 44.1/48 kHz, AAC (default) or Opus; live level meters in settings.
- Devices that disappear record silence and reconnect automatically.

**Saving**

- Hotkeys for 15/30/60 s, custom length (or whole buffer), manual recording, pause, open last clip, open editor.
- Configurable folder and filename template (`ClipDeck_{date}_{time}`, `{source}`, `{duration}`).
- MP4 or MKV; instant copy or re-encode-on-save.
- Post-save actions: open editor, open folder, copy path, run a command.

**Editor**

- Frame-accurate scrubbing and stepping, draggable trim handles + numeric fields, waveform, keyframe ticks.
- Crop (free, 16:9, 9:16, 1:1, 4:3), speed 0.25×–4×, audio fade in/out.
- Per-track keep/remove/mute/volume ("recorded mix" or "per track" mixing).
- PNG screenshot of the current frame, undo/redo (Ctrl+Z / Ctrl+Y).

**Export & compression**

- MP4 / MKV / WebM; H.264, HEVC, AV1; NVENC → AMF → QSV → CPU (x264/x265/SVT-AV1) fallback.
- Quality (CRF/CQ), bitrate (avg/peak) or **target file size** (“8 MB”, “25”, “1.5 GB”) with an
  automatic second attempt if the first one overshoots.
- **Stream-copy export** when only trimming (starts at the previous keyframe; shown in the UI).
- Live size estimate, progress with fps/ETA, cancel, clear errors.

---

## Building

### Prerequisites (Windows 10/11 x64)

| Tool                           | Version                       | Notes                                                                        |
| ------------------------------ | ----------------------------- | ---------------------------------------------------------------------------- |
| Rust                           | stable, MSVC target           | `winget install Rustlang.Rustup`                                             |
| Visual Studio Build Tools 2022 | C++ workload + Windows 11 SDK | components `VC.Tools.x86.x64`, `Windows11SDK.26100`                          |
| LLVM                           | any recent                    | provides `libclang.dll` for the FFmpeg bindings (`winget install LLVM.LLVM`) |
| Node.js                        | 22+                           |                                                                              |
| pnpm                           | 10+                           | `npm i -g pnpm`                                                              |
| PowerShell                     | 7+                            | for the helper scripts                                                       |
| WebView2 Runtime               | evergreen                     | preinstalled on Windows 11                                                   |

`LIBCLANG_PATH` defaults to `C:\Program Files\LLVM\bin` (see `.cargo/config.toml`; override via the environment).

### Steps

```powershell
pnpm install
pnpm fetch:ffmpeg          # downloads + verifies FFmpeg 8.1 (GPL, shared) into third_party/ffmpeg
pnpm tauri dev             # run the app with hot reload
```

### Scripts

| Command                                        | What it does                                       |
| ---------------------------------------------- | -------------------------------------------------- |
| `pnpm dev`                                     | Vite dev server only (UI without the Rust backend) |
| `pnpm build`                                   | Type-check + production frontend build             |
| `pnpm tauri dev`                               | Full app in development mode                       |
| `pnpm tauri build`                             | Release build + NSIS installer                     |
| `pnpm lint` / `pnpm format` / `pnpm typecheck` | ESLint (strict, type-checked) / Prettier / `tsc`   |
| `pnpm cargo:check` / `pnpm cargo:test`         | `cargo check` / `cargo test`                       |
| `pnpm cargo:clippy` / `pnpm cargo:fmt`         | clippy with `-D warnings` / rustfmt                |
| `pnpm bindings`                                | Regenerate TypeScript IPC types from Rust (ts-rs)  |
| `pnpm check`                                   | Everything CI runs                                 |
| `pnpm package:portable`                        | Portable folder + zip in `release/`                |

Plain cargo works too (from the repo root or `src-tauri`): `cargo check`, `cargo test`.

### Tests

```powershell
pnpm cargo:test                                                   # unit + media integration tests
cargo test --manifest-path src-tauri/Cargo.toml --test capture_e2e -- --ignored --test-threads=1
```

Unit tests cover buffer duration/eviction maths, timestamp alignment (session clock, pauses,
audio/video sync), trim/keyframe boundary handling, target-size bitrate estimation, settings
serialization/validation/repair, hotkey parsing, the audio mixer and the export planner.
`tests/export_media.rs` synthesizes a real clip and runs stream-copy / re-encode / target-size
exports, screenshots and waveforms. `tests/capture_e2e.rs` (ignored by default; needs a display,
GPU and audio output) runs the full pipeline live: display + desktop audio (codec, duration, A/V
alignment), downscaled region, a window re-found by exe + title, a disk-backed buffer (eviction and
segment-file cleanup), pause/resume (paused time excluded, A/V aligned) and manual recording to MP4.

---

## Installer & portable builds

**Installer:** `pnpm tauri build` → `src-tauri/target/release/bundle/nsis/ClipDeck_<version>_x64-setup.exe`.
Per-user install (no admin rights), Start-menu entry, uninstaller. FFmpeg DLLs are installed next
to the executable. WebView2 is installed by the bootstrapper if missing.

**Portable:** `pnpm package:portable` → `release/ClipDeck-<version>-portable-x64.zip`. Unzip anywhere
and run `clipdeck.exe`. A file named `portable` next to the exe switches ClipDeck to portable mode:
settings, logs, cache and the WebView2 profile are stored in `.\data` and nothing is written to
`%APPDATA%`/`%LOCALAPPDATA%`. (Clips go to `Videos\ClipDeck` unless you change the output folder.)

ClipDeck never updates itself and does not start with Windows unless you enable it
(Settings → Advanced).

---

## First run

On first launch a short setup appears in the main window:

1. **Display** — pick the monitor to record (window/region capture can be chosen later).
2. **Audio** — desktop output device; microphone is _off_ unless you enable it.
3. **Buffer** — how much to keep (15 s – 5 min presets) and quality.
4. **Save hotkey** — defaults to **Ctrl+Shift+F6** (save last 30 s).

“Start buffering” saves the choices and starts the buffer. Afterwards ClipDeck starts minimized in
the tray (configurable). The red tray dot means “buffering”; left-click opens the window.

---

## How the replay buffer works

```
 Display/Window ──WGC──▶ BGRA texture ──D3D11 video processor──▶ NV12 (scaled/cropped)
                                                                     │ (same GPU texture)
                                                                     ▼
 Desktop audio ─WASAPI loopback─┐                          NVENC / AMF  (or CPU via read-back)
 Microphone ────WASAPI capture──┼─▶ mixer (aligned to session clock) ─▶ AAC/Opus encoders
 Extra inputs ──────────────────┘                                     │
                                                                     ▼
                        ReplayBuffer: encoded packets in ~2 s segments (RAM or disk files)
                        - each segment starts at a video keyframe (keyframe every 1 s)
                        - oldest segments dropped once the rest still covers N s (or size cap hit)
                                                                     │  hotkey
                                                                     ▼
                        snapshot → keyframe at/before (now − N s) → remux to MP4/MKV (no re-encode)
```

- **One clock for everything.** Video ticks and WASAPI packets both carry QPC timestamps and are
  mapped onto a shared _media time_ (microseconds since start, pauses removed). Video is constant
  frame rate: frame _n_ has time _n/fps_; unchanged screens repeat the previous frame.
- **Audio alignment.** Each source has a sample timeline: small jitter (≤20 ms) is ignored, gaps
  (loopback is silent when nothing plays) are filled with silence, overlaps are trimmed. The mixer
  runs 150 ms behind real time so late packets still land in place.
- **Rolling, not “record forever”.** Material older than the buffer length is discarded
  continuously; a 30 s buffer uses ~50–200 MB depending on resolution and content.
- **Saving** waits (≤0.7 s) for audio to catch up to “now”, freezes the needed segments (segment
  files are deleted only when no save still references them), and writes the clip to a temporary
  name before renaming it, so other apps never see half-written files.
- **Manual recording** taps the same packet stream into a crash-safe MKV and remuxes it to MP4 on stop.

---

## Hotkeys

Defaults (change in Settings → Hotkeys or see [`config/hotkeys.example.json`](config/hotkeys.example.json)):

| Action                                     | Default        |
| ------------------------------------------ | -------------- |
| Save last 15 seconds                       | Ctrl+Shift+F5  |
| Save last 30 seconds                       | Ctrl+Shift+F6  |
| Save last 60 seconds                       | Ctrl+Shift+F7  |
| Save custom length (default: whole buffer) | Ctrl+Shift+F8  |
| Start / stop manual recording              | Ctrl+Shift+F9  |
| Pause / resume buffer                      | Ctrl+Shift+F10 |
| Open last clip (default player)            | Ctrl+Shift+F11 |
| Open editor (last clip)                    | Ctrl+Shift+F12 |

Defaults avoid NVIDIA ShadowPlay (Alt+F9/F10/Z), Xbox Game Bar (Win+Alt+…) and AMD Adrenalin
(Ctrl+Shift+letters). Shortcuts are validated in Rust: a modifier is required except for F-keys,
Pause, ScrollLock and PrintScreen; Alt+F4 etc. are rejected; duplicates are refused; registration
failures (another app owns the shortcut) are shown per action. Editor shortcuts: Space play,
←/→ frame step (Shift: 1 s), I/O set in/out, Home/End, Ctrl+Z/Ctrl+Y.

---

## Windows APIs & permissions

ClipDeck runs as a normal user — **no administrator rights and no drivers**.

| Feature                    | API                                                 | Requirement                                                                                                   |
| -------------------------- | --------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| Display/window capture     | Windows.Graphics.Capture                            | Windows 10 1903+; borderless capture (no yellow frame) and capture rate limiting on Windows 11                |
| Display capture fallback   | DXGI Desktop Duplication                            | any Windows 10/11                                                                                             |
| GPU conversion             | D3D11 video processor                               | any D3D11 GPU                                                                                                 |
| Encoding                   | FFmpeg: NVENC / AMF / QSV / x264 / x265 / SVT-AV1   | current GPU driver for hardware encoders                                                                      |
| Audio                      | WASAPI shared mode (loopback for outputs)           | microphone: _Settings → Privacy & security → Microphone → Let desktop apps access your microphone_ must be on |
| Device changes             | `IMMNotificationClient`                             | —                                                                                                             |
| Global hotkeys             | `RegisterHotKey` (via tauri-plugin-global-shortcut) | games running **as administrator** only see hotkeys from elevated apps — run ClipDeck elevated in that case   |
| HUD hidden from recordings | `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)`  | Windows 10 2004+                                                                                              |
| UI                         | WebView2                                            | evergreen runtime                                                                                             |

Window capture shows a yellow border on Windows 10 (a Windows privacy indicator; it is not recorded).

---

## Settings & files

| What                        | Installed                          | Portable               |
| --------------------------- | ---------------------------------- | ---------------------- |
| Settings                    | `%APPDATA%\ClipDeck\settings.json` | `.\data\settings.json` |
| Logs (5 × 5 MB, rotated)    | `%LOCALAPPDATA%\ClipDeck\logs`     | `.\data\logs`          |
| Buffer segments, thumbnails | `%LOCALAPPDATA%\ClipDeck\cache`    | `.\data\cache`         |
| Clips                       | `Videos\ClipDeck` (configurable)   | same                   |

All defaults: [`config/default-settings.json`](config/default-settings.json) (generated from the
Rust defaults; a test keeps it in sync). Invalid or partially broken settings files are repaired
section by section on startup; a corrupt file is kept as `settings.json.corrupt-<timestamp>`.

---

## Known limitations

- **Exclusive fullscreen:** Windows' capture APIs can't see true exclusive-fullscreen games (most
  modern DX11/DX12 games use “fullscreen optimizations” and work fine). ClipDeck detects black
  frames and suggests borderless/windowed mode. Game-hook injection is deliberately not used (anti-cheat risk).
- **HDR:** recorded as SDR; colours of HDR content can look different.
- **Desktop Duplication fallback** does not include the mouse cursor.
- **Region capture** is limited to one display.
- **Overlay HUD** can't appear over exclusive-fullscreen games (the save sound still plays).
- **Preview:** the editor previews H.264 everywhere; HEVC/AV1 preview needs the corresponding
  Windows video extensions. Trimming/exporting works regardless.
- **Audio clock drift** between devices is corrected by small resyncs; on multi-hour sessions a
  rare tiny click is possible. Per-application audio capture is not implemented yet.
- **AMD AMF / Intel QSV** are implemented but untested on real hardware here; CPU fallback is automatic.
- **Size:** the FFmpeg GPL build adds ~190 MB of DLLs (installer: 57 MB, portable zip: 81 MB).
  A trimmed custom FFmpeg build would bring this to ~30–40 MB.
- Windows only for now; capture/audio modules are isolated behind `#[cfg(windows)]` so Linux
  backends (PipeWire, VA-API) can be added later.

---

## Architecture

```
src/                      React + TypeScript UI (control panel only)
  lib/tauri/              typed IPC: commands.ts, events.ts, types.ts (+ generated bindings/)
  features/               clips, capture, audio, hotkeys, settings, editor, export, hud
  stores/                 Zustand store (settings, status, notices)
src-tauri/src/            Rust capture engine
  capture/                WGC + DXGI sources, D3D11 conversion, video pipeline thread
  audio/                  WASAPI sources, mixer, RNNoise, audio engine
  buffer/                 session clock + segmented packet ring buffer
  encode/                 encoder catalog/probe, video & audio encoders
  export/                 remux (instant save), planner, transcoder, frames, export queue
  engine/                 session lifecycle, saving, manual recording, supervisor
  hotkeys/ settings/      validated accelerators, settings model + JSON store
  av/                     RAII wrappers over FFmpeg (ffmpeg-sys-next) + D3D11 hw contexts
  commands.rs events.rs   the IPC surface
  tray.rs ui.rs           tray menu, windows (main, editor, HUD, region picker)
```

The webview never captures, encodes, writes files or registers hotkeys; it calls typed commands
and receives typed events. Raw frames never cross IPC — the editor plays saved files through
Tauri's asset protocol (scoped to the clips folder and opened files). See
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

---

## License

ClipDeck is licensed under the **GNU GPL v3.0 or later** (see [LICENSE](LICENSE)). It links
FFmpeg's GPL build (including x264/x265); see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)
for FFmpeg source availability and other notices.
