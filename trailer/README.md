# Vido trailer

**[Vido-trailer.mp4](Vido-trailer.mp4)**: a 60-second trailer (1920×1080, 30 fps) with voice-over
and music.

Everything in it is made here, from code:

- The **UI** is Vido's real interface, captured from [mock.html](mock.html): the app running on a
  fake backend with fictional games and no real files. The screenshots are kept in
  [shots/](shots).
- The **gameplay** is stand-in art (landscapes with a game-style HUD), drawn by
  [make_frames.py](make_frames.py).
- The **animation** is [trailer.html](trailer.html). It can draw any moment with `render(t)`, so
  the video is rendered frame by frame and comes out the same every time.
- The **music** and **sound effects** are synthesised by [music.py](music.py) and [sfx.py](sfx.py)
  (no samples, nothing to license), plus Vido's own save sound.
- The **voice-over** is [Kokoro](https://github.com/hexgrad/kokoro) (Apache-2.0, voice
  `af_bella`). It runs locally through kokoro-onnx; nothing is sent anywhere.

## Build

Needs Node, Python 3 with Pillow, Microsoft Edge and the FFmpeg dev build
(`pwsh scripts/fetch-ffmpeg.ps1`).

```powershell
pwsh trailer/build.ps1 -Setup     # once: voice model into trailer/.tts (~350 MB)
pwsh trailer/build.ps1            # renders trailer/Vido-trailer.mp4 (a few minutes)
pwsh trailer/build.ps1 -Preview   # quick look: stills sheet in trailer/build/preview.png
pwsh trailer/build.ps1 -Capture   # after UI changes: re-take the screenshots in shots/
```

Intermediate files go to `trailer/build/` (ignored by git).

## Changing it

| What               | Where                                                                                                                                       |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------- |
| Scenes and timing  | `SCENES` in [trailer.html](trailer.html): `[scene, start, end, designed length]`. Each scene is animated for its designed length and sped up to fit. |
| On-screen text     | The scene sections in [trailer.html](trailer.html)                                                                                         |
| Narration          | `LINES` in [voice.py](voice.py) (start time, text). The build stops if two lines would overlap.                                           |
| Voice              | `VOICE` in [voice.py](voice.py) (e.g. `af_heart`, `am_michael`)                                                                            |
| Music              | `build.ps1` calls `music.py ... lofi 60`; styles are `chill`, `lofi` and `synthwave`                                                       |
| Sound effects      | `SFX` in [mix.py](mix.py); keep `SCENES` there in sync with trailer.html                                                                   |
| UI screenshots     | [capture-steps.json](capture-steps.json) (clicks, drags and shots in the mock), then `-Capture`                                            |

The page also has scenes that aren't in the 60-second cut (library, privacy blur, quick formats).
Add them back to `SCENES` to use them. To preview the page in a browser, serve this folder the way
`render.mjs` does, then open `trailer.html?t=12` (one moment) or `trailer.html?play` (real time).
