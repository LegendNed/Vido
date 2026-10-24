# Trailer music in three styles plus three scene-transition sounds, synthesised
# from scratch (no samples, nothing to license).
# The trailer uses lo-fi without transition sounds (build.ps1).
# usage: python music.py <out.wav> <style: chill|lofi|synthwave> <seconds> [transition: none|swell|chime] [cut,cut,...]
import array
import math
import random
import sys
import wave

out, style, DUR = sys.argv[1], sys.argv[2], float(sys.argv[3])
TRANS = sys.argv[4] if len(sys.argv) > 4 else "none"
CUTS = [float(c) for c in sys.argv[5].split(",")] if len(sys.argv) > 5 and sys.argv[5] else []
SR = 32000
N = int(DUR * SR)
mix = array.array("f", bytes(4 * N))
random.seed(7)


def hz(n):
    return 440.0 * 2 ** ((n - 69) / 12)


def add(start, seg, gain=1.0):
    i0 = int(start * SR)
    for k, v in enumerate(seg):
        i = i0 + k
        if 0 <= i < N:
            mix[i] += v * gain


def lowpass(seg, a):
    y = 0.0
    for k in range(len(seg)):
        y += a * (seg[k] - y)
        seg[k] = y
    return seg


def tone(f, dur, decay, kind="sine", attack=0.005):
    n = int(dur * SR)
    w = 2 * math.pi * f / SR
    s = []
    for k in range(n):
        t = k / SR
        env = min(1.0, t / attack) * math.exp(-t * decay)
        ph = w * k
        if kind == "sine":
            x = math.sin(ph)
        elif kind == "tri":
            x = (2 / math.pi) * math.asin(math.sin(ph))
        elif kind == "saw":  # band-limited-ish saw: a few harmonics
            x = sum(math.sin(h * ph) / h for h in range(1, 7)) * 0.6
        elif kind == "ep":  # electric-piano-ish: sine + soft bell partial + tremolo
            x = (math.sin(ph) + 0.3 * math.sin(2 * ph) * math.exp(-t * 6) + 0.12 * math.sin(3.5 * ph) * math.exp(-t * 9)) * (1 + 0.12 * math.sin(2 * math.pi * 4.5 * t))
        s.append(x * env)
    return s


def pad(chord, dur, lp=0.08, detune=(-0.12, 0.0, 0.11), harm=0.18):
    n = int((dur + 0.6) * SR)
    seg = [0.0] * n
    for note in chord:
        for d in detune:
            w = 2 * math.pi * hz(note + d) / SR
            ph0 = random.random() * 6.28
            for k in range(n):
                p = ph0 + w * k
                seg[k] += math.sin(p) + harm * math.sin(2 * p)
    seg = lowpass(seg, lp)
    for k in range(n):
        t = k / SR
        seg[k] *= min(1.0, t / 0.5) * (1.0 if t < dur else max(0.0, 1 - (t - dur) / 0.6))
    return seg


def kick(f0=140, f1=45, decay=11, dur=0.3):
    s, ph = [], 0.0
    for k in range(int(dur * SR)):
        t = k / SR
        ph += 2 * math.pi * (f1 + (f0 - f1) * math.exp(-t * 28)) / SR
        s.append(math.sin(ph) * math.exp(-t * decay))
    return s


def noise(dur, decay, lp=None, hp=False):
    s, prev = [], 0.0
    for k in range(int(dur * SR)):
        r = random.uniform(-1, 1)
        if hp:
            x, prev = r - prev, r
        else:
            x = r
        s.append(x * math.exp(-k / SR * decay))
    return lowpass(s, lp) if lp else s


def fade_in_out(t):
    return min(1.0, t / 1.0)


if style == "chill":  # 120 BPM, Am F C G, pad + plucked bass + kick + hats + arp
    BEAT, BAR = 0.5, 2.0
    CH = [[57, 60, 64, 69], [53, 57, 60, 65], [48, 55, 60, 64], [55, 59, 62, 67]]
    RT = [45, 41, 48, 43]
    for b in range(int(DUR / BAR) + 1):
        add(b * BAR, pad(CH[b % 4], BAR), 0.018)
        for off in (0.0, 1.0, 1.75):
            add(b * BAR + off, [x + 0.25 * y for x, y in zip(tone(hz(RT[b % 4] - 12), 0.45, 6), tone(hz(RT[b % 4]), 0.45, 12))], 0.2)
    t = 0.0
    while t < DUR:
        add(t, kick(), 0.28)
        add(t + BEAT / 2, noise(0.06, 70, hp=True), 0.05)
        t += BEAT
    t, step = 0.0, 0
    while t < DUR:
        ch = CH[int(t / BAR) % 4]
        add(t, tone(hz(ch[[0, 2, 1, 3, 2, 1, 3, 2][step % 8]] + 12), 0.22, 14, "tri"), 0.03)
        t += BEAT / 2
        step += 1
elif style == "lofi":  # 82 BPM, jazzy 7ths, e-piano, soft swung drums, vinyl
    BEAT = 60 / 82
    BAR = 4 * BEAT
    CH = [[53, 57, 60, 64], [52, 55, 59, 62], [50, 53, 57, 60], [48, 52, 55, 59]]  # Fmaj7 Em7 Dm7 Cmaj7
    RT = [41, 40, 38, 36]
    for b in range(int(DUR / BAR) + 1):
        t0 = b * BAR
        for j, note in enumerate(CH[b % 4]):
            add(t0 + 0.012 * j, tone(hz(note), BAR * 0.95, 1.1, "ep", attack=0.01), 0.05)
            add(t0 + 2 * BEAT + 0.15 + 0.012 * j, tone(hz(note), BEAT * 1.6, 1.6, "ep", attack=0.01), 0.03)
        add(t0, tone(hz(RT[b % 4] - 12), BAR * 0.6, 2.2), 0.22)
        add(t0 + 2.5 * BEAT, tone(hz(RT[b % 4] - 12), BEAT, 3), 0.16)
        for k, swing in enumerate([0, 0.62, 1, 1.62, 2, 2.62, 3, 3.62]):
            tt = t0 + swing * BEAT
            if k in (0, 5):
                add(tt, kick(110, 48, 9), 0.26)
            if k in (2, 6):
                add(tt, noise(0.16, 18, lp=0.35), 0.10)
            add(tt, noise(0.04, 90, hp=True), 0.025)
    # Whole track a bit muffled.
    lowpass(mix, 0.35)
elif style == "synthwave":  # 105 BPM, driving 8th saw bass, claps, bright 16th arp
    BEAT = 60 / 105
    BAR = 4 * BEAT
    CH = [[57, 60, 64], [53, 57, 60], [48, 52, 55], [55, 59, 62]]
    RT = [33, 29, 36, 31]
    for b in range(int(DUR / BAR) + 1):
        t0 = b * BAR
        add(t0, pad([n + 12 for n in CH[b % 4]], BAR, lp=0.12, harm=0.4), 0.02)
        for e in range(8):
            add(t0 + e * BEAT / 2, lowpass(tone(hz(RT[b % 4] + (12 if e % 2 else 0)), BEAT / 2 * 0.9, 5, "saw"), 0.25), 0.14)
        for q in range(4):
            add(t0 + q * BEAT, kick(150, 50, 10), 0.32)
            if q in (1, 3):
                add(t0 + q * BEAT, noise(0.18, 22, lp=0.6), 0.11)
        for s in range(16):
            note = CH[b % 4][[0, 1, 2, 1][s % 4]] + 24
            add(t0 + s * BEAT / 4, lowpass(tone(hz(note), BEAT / 4, 12, "saw"), 0.4), 0.02)
else:
    sys.exit(f"unknown style {style}")

# Scene transitions.
for c in CUTS:
    if TRANS == "swell":  # quiet, low-passed air, short
        n = int(0.6 * SR)
        seg = lowpass([random.uniform(-1, 1) for _ in range(n)], 0.03)
        seg = [v * math.sin(math.pi * k / n) ** 2 for k, v in enumerate(seg)]
        add(c - 0.45, seg, 0.35)
    elif TRANS == "chime":  # soft two-note bell at the cut
        add(c, [a + 0.5 * b for a, b in zip(tone(hz(88), 0.9, 4.5), tone(hz(95), 0.9, 6))], 0.05)
    elif TRANS == "loud":  # the first version (for comparison)
        n = int(0.9 * SR)
        seg, prev = [], 0.0
        for k in range(n):
            prev += 0.05 * (random.uniform(-1, 1) - prev)
            seg.append(prev * math.sin(math.pi * k / n) ** 2)
        add(c - 0.75, seg, 0.5)

peak = max(abs(v) for v in mix) or 1
pcm = array.array("h")
for i, v in enumerate(mix):
    t = i / SR
    # Smooth (equal-power) fade in over 1.5 s and out over 3 s.
    fade = math.sin(min(1.0, t / 1.5) * math.pi / 2) * math.sin(min(1.0, max(0.0, (DUR - t) / 3.0)) * math.pi / 2)
    pcm.append(int(math.tanh(1.3 * v / peak) * 0.75 * fade * 32767))
with wave.open(out, "wb") as w:
    w.setnchannels(1)
    w.setsampwidth(2)
    w.setframerate(SR)
    w.writeframes(pcm.tobytes())
print("ok", out)
