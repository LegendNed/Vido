# Mixes the voice-over, the music (ducked under the voice) and the sound effects
# onto the rendered video.
# usage: python mix.py <build_dir> <out.mp4>
import json
import os
import subprocess
import sys

here = os.path.dirname(os.path.abspath(__file__))
repo = os.path.dirname(here)
FF = os.path.join(repo, "third_party", "ffmpeg", "bin", "ffmpeg.exe")
SAVE = os.path.join(repo, "config", "sounds", "two-tone.wav")  # Vido's default save sound
build, out = sys.argv[1], os.path.abspath(sys.argv[2])
audio = os.path.join(build, "audio")

# Scene timing, as in SCENES in trailer.html: [start, end, designed length].
# Sound effects are placed at scene-local times of the designed timeline.
SCENES = {"s2": (3.5, 11, 10), "s3": (11, 19, 11), "s4": (19, 25.5, 8), "s5": (25.5, 32, 8)}


def at(scene, local):
    a, b, length = SCENES[scene]
    return a + local * (b - a) / length


CLICK, POP = os.path.join(audio, "click.wav"), os.path.join(audio, "pop.wav")
# (file, time in seconds, gain)
SFX = [
    (CLICK, at("s2", 5.0), 0.7), (SAVE, at("s2", 5.12), 0.8),  # save hotkey
    (CLICK, at("s3", 4.0), 0.7), (SAVE, at("s3", 4.1), 0.8),  # first save
    (CLICK, at("s3", 6.5), 0.7), (SAVE, at("s3", 6.65), 0.8),  # "make it longer"
    (CLICK, at("s4", 2.0), 0.7), (POP, at("s4", 2.15), 0.6), (POP, at("s4", 5.7), 0.6),  # paste, drop
    (SAVE, at("s5", 5.6), 0.8),  # re-encoded clip ready (the only sound in that scene)
]
voice = json.load(open(os.path.join(audio, "voice", "plan.json")))

args = [FF, "-hide_banner", "-loglevel", "error", "-y", "-i", os.path.join(build, "video.mp4"), "-i", os.path.join(audio, "music.wav")]
f = ["[1:a]aresample=48000,volume=0.95[m]"]
idx = 2
sfx_labels = []
for path, t, gain in SFX:
    args += ["-i", path]
    f.append(f"[{idx}:a]aresample=48000,volume={gain},adelay={int(t * 1000)}:all=1[s{idx}]")
    sfx_labels.append(f"[s{idx}]")
    idx += 1
vo_labels = []
for v in voice:
    args += ["-i", os.path.join(audio, "voice", v["file"])]
    f.append(f"[{idx}:a]aresample=48000,adelay={int(v['start'] * 1000)}:all=1[v{idx}]")
    vo_labels.append(f"[v{idx}]")
    idx += 1
# Voice bus: trim rumble, a little presence, then split for ducking.
f.append(f"{''.join(vo_labels)}amix=inputs={len(vo_labels)}:normalize=0:duration=longest,"
         "highpass=f=80,equalizer=f=3000:t=q:w=1.2:g=2,apad[vo]")
f.append("[vo]asplit=2[vo1][vo2]")
# The music dips while she speaks and recovers smoothly.
f.append("[m][vo1]sidechaincompress=threshold=0.02:ratio=6:attack=30:release=600:makeup=1[md]")
f.append(f"[md]{''.join(sfx_labels)}[vo2]amix=inputs={2 + len(sfx_labels)}:normalize=0:duration=first,"
         "loudnorm=I=-15:TP=-1.5:LRA=9,aresample=48000[a]")
args += ["-filter_complex", ";".join(f), "-map", "0:v", "-map", "[a]", "-c:v", "copy",
         "-c:a", "aac", "-b:a", "192k", "-ac", "2", "-shortest", "-movflags", "+faststart", out]
subprocess.run(args, check=True)
print("trailer:", out)
