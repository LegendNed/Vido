# How Vido works

## The replay buffer

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
- **Rolling, not "record forever".** Material older than the buffer length is discarded
  continuously; a 30 s buffer uses ~50–200 MB depending on resolution and content.
- **Low overhead.** Captured frames stay on the GPU: colour conversion and scaling run in the D3D11
  video processor and the hardware encoder reads the same texture (zero CPU copies). The CPU
  encoders get a read-back copy.

## Saving

- **Instant save** waits (≤0.7 s) for audio to catch up to "now", freezes the needed segments
  (segment files are deleted only when no save still references them), and remuxes them into the
  clip under a temporary name before renaming it, so other apps never see half-written files.
- **Game folders.** _Is it a game?_ The foreground program counts as one when Windows' Game Bar
  recognised it (`HKCU\System\GameConfigStore`, the list behind Win+G) or when it runs from a
  folder that Steam (`libraryfolders.vdf`, `appmanifest_*.acf`), Epic (`.item` manifests), GOG, EA
  app or Ubisoft Connect (registry) installed. Window titles and exe names never decide this.
  _What is it called?_ The first of: the library's official name; the name remembered for that
  exe in `games.json` (so a game keeps one folder even when its window title changes); built-in
  names for odd cases (Minecraft runs as `javaw.exe`); the window title without version numbers;
  the exe's product name (unless it names an engine such as Unreal or OpenJDK); the game's folder
  (skipping `Binaries\Win64`, Unreal project code names and version suffixes); the exe name.
  Nothing is fetched online. A background check every 3 s remembers the last game, so a clip saved
  from the tray still gets it. Clips of a game go to `<clips folder>\<game>\` (also while staged
  for re-encoding), others to the clips folder. `{game}` in the filename template is the game's
  name ("Clip" without one).
- Each clip gets a **mix track** (what players play) plus **one track per audio source** for the
  editor.
- **Re-encode on save** writes the instant save to `cache\pending` instead of the clips folder,
  re-encodes it there with the Saving page's options and writes only the result to the clips
  folder; the staged file is then deleted. The first popup says "re-encoding…"; the save sound,
  clipboard copy, post-save actions and the "Clip ready" popup follow once the re-encoded clip
  exists. If re-encoding fails or is cancelled, the staged clip is moved to the clips folder as is;
  leftovers from a crash are moved there on the next start.
- **Manual recording** taps the same packet stream into a crash-safe MKV and remuxes it to MP4 on
  stop.
- The clip lists follow the clips folder: files added, deleted or renamed outside Vido show up
  within about half a second.

## Editor and export

- **Save / Save & override** use the export pane's options. "Original" stream-copies the video
  when only trimming (starting at the keyframe at or before the in point); any other preset
  re-encodes. Save & override keeps the clip's container, writes a temporary
  `<name>.vido-tmp.<ext>` next to it, then moves the original to the Recycle Bin and gives the
  edit its name and date.
- **Target size** exports estimate the bitrate from the duration and audio, and retry with a
  corrected bitrate if the first pass overshoots.
- **Encoders:** NVENC → AMF → QSV → CPU (x264/x265/SVT-AV1), probed once and cached.
- **Live lane preview:** each audio track plays from its own copy (no re-encode, cached in
  `cache\preview`, cleaned after a day) through a Web Audio gain, so mute/volume changes are heard
  immediately.

## Updates

- Tauri's updater reads `latest.json` from the newest GitHub release (shortly after start, then
  every 6 hours, while Settings → General → "Check for updates automatically" is on). A newer
  version only shows the top bar's **Update** button.
- Every package is verified against the public key built into Vido before it is installed
  (minisign signatures made in the release workflow).
- **Installed copies** run the NSIS installer in passive mode; it replaces Vido and starts it
  again. **Portable copies** download the signed zip and swap their files in place: running
  files can be renamed but not overwritten, so each replaced file becomes `<name>.vido-old`
  (put back if anything fails, deleted on the next start), `data\` and the `portable` marker are
  kept, and Vido restarts itself.
- With "Install updates automatically" on, the update is downloaded in the background to
  `cache\update` and installed the next time Vido starts, before the buffer starts.

## Windows APIs & permissions

Vido runs as a normal user: **no administrator rights and no drivers**.

| Feature                      | API                                                       | Requirement                                                                                                   |
| ---------------------------- | --------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| Display/window capture       | Windows.Graphics.Capture                                  | Windows 10 1903+; borderless capture (no yellow frame) and capture rate limiting on Windows 11                |
| Display capture fallback     | DXGI Desktop Duplication                                  | any Windows 10/11                                                                                             |
| GPU conversion               | D3D11 video processor                                     | any D3D11 GPU                                                                                                 |
| Encoding                     | FFmpeg: NVENC / AMF / QSV / x264 / x265 / SVT-AV1         | current GPU driver for hardware encoders                                                                      |
| Audio                        | WASAPI shared mode (loopback for outputs)                 | microphone: _Settings → Privacy & security → Microphone → Let desktop apps access your microphone_ must be on |
| Device changes               | `IMMNotificationClient`                                   | -                                                                                                             |
| Global hotkeys               | `RegisterHotKey` (via tauri-plugin-global-shortcut)       | games running **as administrator** only see hotkeys from elevated apps; run Vido elevated in that case        |
| Overlay hidden from captures | `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)`        | Windows 10 2004+                                                                                              |
| Clips folder watching        | `FindFirstChangeNotificationW`                            | -                                                                                                             |
| Save sound                   | `PlaySoundW` (from memory, volume applied to the samples) | -                                                                                                             |
| UI                           | WebView2 (undecorated window with its own title bar)      | evergreen runtime                                                                                             |
| Save & override              | `SHFileOperationW` (Recycle Bin)                          | drives without a Recycle Bin ask before deleting the original                                                 |

Window capture shows a yellow border on Windows 10 (a Windows privacy indicator; it is not
recorded).

## Known limitations

- **Exclusive fullscreen:** Windows' capture APIs can't see true exclusive-fullscreen games (most
  modern DX11/DX12 games use "fullscreen optimizations" and work fine). Vido detects black frames
  and suggests borderless/windowed mode. Game-hook injection is deliberately not used
  (anti-cheat risk).
- **Overlay and popups** can't appear over exclusive-fullscreen games (borderless and windowed
  work; the save sound still plays).
- **HDR:** recorded as SDR; colours of HDR content can look different.
- **Desktop Duplication fallback** does not include the mouse cursor.
- **Region capture** is limited to one display.
- **Preview:** the editor previews H.264 everywhere; HEVC/AV1 preview needs the corresponding
  Windows video extensions. Trimming and saving work regardless.
- **Audio clock drift** between devices is corrected by small resyncs; on multi-hour sessions a
  rare tiny click is possible. Per-application audio capture is not implemented yet.
- **Hardware:** tested on NVIDIA (RTX 5090: NVENC H.264/HEVC/AV1) and AMD (AMF). Intel Quick Sync
  is implemented through FFmpeg but less tested; the CPU fallback is automatic.
- **Size:** the FFmpeg GPL build adds ~190 MB of DLLs (installer: 57 MB, portable zip: 81 MB). A
  trimmed custom FFmpeg build would bring this to ~30–40 MB.
- Windows only for now; capture/audio modules are isolated behind `#[cfg(windows)]` so Linux
  backends (PipeWire, VA-API) can be added later.
