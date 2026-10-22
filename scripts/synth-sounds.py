# Synthesizes the built-in "clip saved" sounds into src-tauri/assets/sounds
# (48 kHz, 16-bit mono WAV; embedded in the exe). Usage: python scripts/synth-sounds.py
import math
import os
import random
import struct
import wave

SR = 48000
OUT = os.path.join(os.path.dirname(__file__), "..", "src-tauri", "assets", "sounds")


def env(t, a, d):
    """Linear attack over `a` seconds, then exponential decay (time constant `d`)."""
    return (t / a if t < a else 1.0) * math.exp(-max(0.0, t - a) / d)


def render(length, fn, level=0.35):
    """Samples `fn` for `length` seconds, normalized to `level` (about -9 dBFS)."""
    n = int(SR * length)
    s = [fn(i / SR) for i in range(n)]
    peak = max(abs(x) for x in s) or 1
    s = [x / peak * level for x in s]
    fade = int(SR * 0.008)
    for i in range(fade):
        s[-1 - i] *= i / fade
    return s


def save(name, s):
    with wave.open(os.path.join(OUT, name + ".wav"), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(b"".join(struct.pack("<h", int(x * 32767)) for x in s))


def note(f, start, t, a=0.008, d=0.12, partials=((1, 1.0),)):
    t -= start
    if t < 0:
        return 0.0
    return env(t, a, d) * sum(g * math.sin(2 * math.pi * f * m * t) for m, g in partials)


def bloop(t, f0=520.0, f1=300.0, sweep=0.14):
    """Sine gliding from f0 to f1 over `sweep` seconds."""
    if t < sweep:
        ph = f0 * t + (f1 - f0) * t * t / (2 * sweep)
    else:
        ph = f0 * sweep + (f1 - f0) * sweep / 2 + f1 * (t - sweep)
    return env(t, 0.006, 0.06) * math.sin(2 * math.pi * ph)


def pluck(f=220.0, length=0.45):
    """Karplus-Strong plucked string."""
    random.seed(3)
    n, p = int(SR * length), int(SR / f)
    buf = [random.uniform(-1, 1) for _ in range(p)]
    s = []
    for i in range(n):
        x = buf[i % p]
        buf[i % p] = 0.5 * (x + buf[(i + 1) % p]) * 0.992
        s.append(x)
    return s


os.makedirs(OUT, exist_ok=True)
# Low wooden "tok" (G4, marimba-like partials that die fast).
save("tap", render(0.25, lambda t: note(392, 0, t, 0.002, 0.07, ((1, 1.0), (3.9, 0.12)))))
# Two gentle sine notes a fourth apart (A4 -> D5): the default.
save("two-tone", render(0.4, lambda t: note(440, 0, t, 0.01, 0.07) + note(587.3, 0.09, t, 0.01, 0.1)))
# Soft sine dipping from 520 to 300 Hz.
save("bloop", render(0.25, bloop))
# Warm plucked string (A3).
pl = pluck()
save("pluck", render(0.45, lambda t: pl[min(int(t * SR), len(pl) - 1)] * env(t, 0.001, 0.15)))
