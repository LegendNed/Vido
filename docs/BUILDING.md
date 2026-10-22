# Building Vido

## Prerequisites (Windows 10/11 x64)

| Tool                           | Version                       | Notes                                                                        |
| ------------------------------ | ----------------------------- | ---------------------------------------------------------------------------- |
| Rust                           | stable, MSVC target           | `winget install Rustlang.Rustup`                                             |
| Visual Studio Build Tools 2022 | C++ workload + Windows 11 SDK | components `VC.Tools.x86.x64`, `Windows11SDK.26100`                          |
| LLVM                           | any recent                    | provides `libclang.dll` for the FFmpeg bindings (`winget install LLVM.LLVM`) |
| Node.js                        | 22+                           |                                                                              |
| pnpm                           | 10+                           | `npm i -g pnpm`                                                              |
| PowerShell                     | 7+                            | for the helper scripts                                                       |
| Python                         | 3.10+                         | only to regenerate the built-in sounds (`scripts/synth-sounds.py`)           |
| WebView2 Runtime               | evergreen                     | preinstalled on Windows 11                                                   |

`LIBCLANG_PATH` defaults to `C:\Program Files\LLVM\bin` (see `.cargo/config.toml`; override via
the environment).

## Steps

```powershell
pnpm install
pnpm fetch:ffmpeg          # downloads + verifies FFmpeg 8.1 (GPL, shared) into third_party/ffmpeg
pnpm tauri dev             # run the app with hot reload
```

## Scripts

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

The built-in save sounds (`config/sounds/*.wav`) are synthesized by
`python scripts/synth-sounds.py` and embedded in the executable.

## Config files

Lists of content and tuning live in `config/` as JSON and are built into the app (the UI
imports them, Rust reads them with `include_str!`); tests check each file against the
settings validation, so a typo fails `cargo test` rather than the app.

| File                     | What it holds                                                                                               |
| ------------------------ | ----------------------------------------------------------------------------------------------------------- |
| `default-settings.json`  | All defaults (generated from Rust; a test keeps it in sync)                                                 |
| `hotkeys.example.json`   | Example hotkey settings                                                                                     |
| `export-presets.json`    | Editor / Saving / Export presets (Discord, Original, 10/25/50 MB, …)                                        |
| `recording-presets.json` | Recording quality cards (Low, Standard, High, Native)                                                       |
| `clip-lengths.json`      | Clip length buttons (Video settings, Overview, setup)                                                       |
| `save-sounds.json`       | Save sounds (ids, labels, files in `config/sounds`) and volume                                              |
| `game-detection.json`    | Built-in game names and the generic names skipped when naming games                                         |
| `idle-phrases.json`      | Sidebar phrases while no game is detected                                                                   |
| `sounds/*.wav`           | The built-in save sounds (from `scripts/synth-sounds.py`)                                                   |
| `icons/`                 | App, installer and sidebar icons (`source.png` → `pnpm tauri icon config/icons/source.png -o config/icons`) |

## Tests

```powershell
pnpm cargo:test                                                   # unit + media integration tests
cargo test --manifest-path src-tauri/Cargo.toml --test capture_e2e -- --ignored --test-threads=1
```

Unit tests cover buffer duration/eviction maths, timestamp alignment (session clock, pauses,
audio/video sync), trim/keyframe boundary handling, target-size bitrate estimation, settings
serialization/validation/repair, hotkey parsing, the audio mixer, the export planner, editor
saves and the save-sound WAV scaling. `tests/export_media.rs` synthesizes a real clip and runs
stream-copy / re-encode / target-size exports, screenshots and waveforms. `tests/capture_e2e.rs`
(ignored by default; needs a display, GPU and audio output) runs the full pipeline live: display +
desktop audio (codec, duration, A/V alignment), downscaled region, a window re-found by exe +
title, a disk-backed buffer (eviction and segment-file cleanup), pause/resume (paused time
excluded, A/V aligned) and manual recording to MP4.

## Installer & portable builds

**Installer:** `pnpm tauri build` → `src-tauri/target/release/bundle/nsis/Vido_<version>_x64-setup.exe`.
Per-user install (no admin rights), Start-menu entry, uninstaller. FFmpeg DLLs are installed next
to the executable. WebView2 is installed by the bootstrapper if missing.

**Portable:** `pnpm package:portable` → `release/Vido-<version>-portable-x64.zip`. Unzip anywhere
and run `vido.exe`. A file named `portable` next to the exe switches Vido to portable mode:
settings, logs, cache and the WebView2 profile are stored in `.\data` and nothing is written to
`%APPDATA%`/`%LOCALAPPDATA%`. (Clips go to `Videos\Vido` unless you change the output folder.)
Repackaging keeps an existing `.\data` folder and leaves it out of the zip.

**Releases:** publishing a GitHub release runs `.github/workflows/release.yml`, which builds both
and attaches them as `Vido-setup.exe` and `Vido-portable-x64.zip` (stable "latest" download
links). CI (`ci.yml`) runs manually via _workflow_dispatch_.

Vido starts with Windows by default (Settings → General).

## Releasing (and the updater)

1. `node scripts/set-version.mjs 0.2.0` (package.json, Cargo.toml, tauri.conf.json), commit, push.
2. Publish a GitHub release tagged `v0.2.0`; its notes become the in-app update notes.
3. `release.yml` builds the installer (with `tauri.release.conf.json`, which turns on the
   updater's `.sig` files) and the portable zip, signs the zip with `tauri signer sign`, writes
   `latest.json` and attaches `Vido-setup.exe`, `Vido-portable-x64.zip` and `latest.json`.

The app checks `releases/latest/download/latest.json`: the `windows-x86_64` entry (installer)
for installed copies, `windows-x86_64-portable` (zip) for portable ones. A package is only
installed when its signature matches the public key in `tauri.conf.json` →
`plugins.updater.pubkey`.

**Signing key.** Made once with `pnpm tauri signer generate -w ~/.tauri/vido-updater.key`; the
private key stays out of the repo. Add its content as the repository secret
`TAURI_SIGNING_PRIVATE_KEY` (and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` if it has one). Losing the
key means existing installs can no longer verify new releases, so keep a backup.
