# Small trailer sound effects, synthesised (no samples): a key click and a pop.
# usage: python sfx.py <out_dir>
import array
import math
import os
import random
import sys
import wave

out = sys.argv[1]
SR = 32000
random.seed(3)
os.makedirs(out, exist_ok=True)


def write(name, samples):
    with wave.open(os.path.join(out, name), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(array.array("h", (int(max(-1, min(1, v)) * 32767) for v in samples)).tobytes())


# Key click: short filtered noise tick + low thump.
click, prev = [], 0.0
for k in range(int(0.05 * SR)):
    prev += 0.4 * (random.uniform(-1, 1) - prev)
    click.append(0.6 * prev * math.exp(-k / SR * 160) + 0.4 * math.sin(2 * math.pi * 180 * k / SR) * math.exp(-k / SR * 80))
write("click.wav", click)

# Pop: a rising sine blip.
pop, ph = [], 0.0
for k in range(int(0.12 * SR)):
    t = k / SR
    ph += 2 * math.pi * (500 + 900 * t / 0.12) / SR
    pop.append(0.5 * math.sin(ph) * math.exp(-t * 30))
write("pop.wav", pop)
print("sfx:", out)
