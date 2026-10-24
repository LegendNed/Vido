# Voice-over with Kokoro (local, offline; voice "af_bella"). One WAV per line, a
# plan.json with the start times for mix.py, and a check that every line ends
# before the next one starts. Run with the Kokoro venv (build.ps1 -Setup).
# usage: python voice.py <model_dir> <out_dir>
import json
import os
import sys

import soundfile as sf
from kokoro_onnx import Kokoro

model_dir, out = sys.argv[1], sys.argv[2]
VOICE = "af_bella"
# (start in seconds, text): each starts just after its moment on screen.
LINES = [
    (0.5, "Meet Vido. Clip the moment you just played."),
    (3.9, "It keeps your last thirty seconds ready."),
    (7.9, "One key, and it's saved. Instantly."),
    (11.4, "Missed the ending? Save again."),
    (16.2, "Both become one longer clip."),
    (19.3, "Paste it into chat, or drag it right out."),
    (25.8, "Or let Vido shrink it to Discord size, in the background."),
    (32.3, "Every device gets its own track, so you can mute your mic later."),
    (39.3, "See what's eating your disk, and shrink old clips in one click."),
    (45.3, "Light. Local. No account, no telemetry."),
    (50.8, "All of the clips. None of the bloat."),
    (55.8, "Vido. Free and open source."),
]
k = Kokoro(os.path.join(model_dir, "kokoro-v1.0.onnx"), os.path.join(model_dir, "voices-v1.0.bin"))
os.makedirs(out, exist_ok=True)
plan = []
for i, (start, text) in enumerate(LINES):
    s, sr = k.create(text, voice=VOICE, speed=1.0, lang="en-us")
    name = f"b{i:02d}.wav"
    sf.write(os.path.join(out, name), s, sr)
    plan.append({"file": name, "start": start, "dur": round(len(s) / sr, 2), "text": text})
bad = [
    f'{a["text"]!r} ends at {a["start"] + a["dur"]:.2f} s, the next line starts at {b["start"]} s'
    for a, b in zip(plan, plan[1:])
    if a["start"] + a["dur"] > b["start"] - 0.15
]
if plan[-1]["start"] + plan[-1]["dur"] > 59.0:
    bad.append("the last line ends after 59 s")
json.dump(plan, open(os.path.join(out, "plan.json"), "w"), indent=1)
for p in plan:
    print(f'{p["start"]:5.1f}  {p["dur"]:4.2f}s  {p["text"]}')
if bad:
    sys.exit("Lines overlap:\n" + "\n".join(bad))
