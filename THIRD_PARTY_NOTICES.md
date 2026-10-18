# Third-party notices

ClipDeck is licensed under the GNU General Public License v3.0 or later (see `LICENSE`).

## FFmpeg

ClipDeck dynamically links FFmpeg (libavcodec, libavformat, libavfilter, libavutil,
libswresample, libswscale), version 8.1, built by
[BtbN/FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds) in its **GPL** variant, which
includes libx264, libx265, SVT-AV1, libopus and others.

- FFmpeg is licensed under the GNU GPL version 3 for this build. See `licenses/FFmpeg-LICENSE.txt`.
- FFmpeg source code: https://ffmpeg.org/download.html (release 8.1) and
  https://github.com/FFmpeg/FFmpeg/tree/release/8.1
- Build scripts used to produce the binaries: https://github.com/BtbN/FFmpeg-Builds
- The exact archive name and SHA-256 are recorded in `third_party/ffmpeg/VERSION.txt`
  when built from source with `scripts/fetch-ffmpeg.ps1`.

## RNNoise (via `nnnoiseless`)

Microphone noise suppression uses `nnnoiseless`, a Rust port of Xiph.Org's RNNoise
(BSD-3-Clause). Copyright (c) 2017 Mozilla, (c) 2007-2017 Jean-Marc Valin, (c) 2005-2017 Xiph.Org Foundation.

## Rust and npm dependencies

Rust crates (Tauri, windows-rs, serde, tracing, ...) and npm packages (React, Zustand,
@tauri-apps/api) are used under their respective MIT / Apache-2.0 / BSD licenses.
Run `cargo tree` and `pnpm licenses list` for the complete list.
