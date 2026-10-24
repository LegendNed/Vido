# Stand-in "gameplay" frames for the trailer and the UI mock: stylised landscapes
# with a game-like HUD (health, ammo, minimap, kill feed with player names, team
# chat). No real footage or real game. usage: python make_frames.py <out_dir> [<mock_assets_dir>]
import math
import os
import random
import sys

from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageFont

out = sys.argv[1]
mock = sys.argv[2] if len(sys.argv) > 2 else None
os.makedirs(out, exist_ok=True)
if mock:
    os.makedirs(mock, exist_ok=True)
W, H = 1920, 1080
FONTS = r"C:\Windows\Fonts"
def font(name, size):
    return ImageFont.truetype(os.path.join(FONTS, name), size)
F_HUD = font("seguisb.ttf", 30)
F_BIG = font("segoeuib.ttf", 64)
F_SM = font("seguisb.ttf", 24)
F_CHAT = font("segoeui.ttf", 24)

pal = [
    ((18, 24, 60), (255, 120, 80), (40, 20, 60), (255, 200, 120)),
    ((10, 30, 50), (60, 200, 220), (10, 50, 60), (200, 255, 255)),
    ((40, 10, 50), (220, 80, 200), (30, 10, 40), (255, 180, 240)),
    ((20, 40, 20), (150, 220, 90), (20, 60, 30), (240, 255, 180)),
    ((60, 30, 10), (255, 170, 60), (70, 35, 15), (255, 230, 160)),
    ((15, 15, 30), (120, 140, 255), (20, 20, 50), (220, 230, 255)),
]
NAMES = ["NightOwl", "xRaven", "KaiTheBold", "Mira_77", "Bramble", "SpudLord", "Juno", "TTV_Vex"]
CHAT = [
    ("[Team] KaiTheBold", "push B, they're low"),
    ("[Team] Mira_77", "nice shot!!"),
    ("[All] SpudLord", "gg ez"),
    ("[Team] Juno", "rotating left"),
]


def landscape(i):
    top, bottom, mount, sun = pal[i]
    random.seed(i * 7 + 1)
    img = Image.new("RGB", (W, H))
    d = ImageDraw.Draw(img)
    for y in range(H):
        t = y / H
        d.line([(0, y), (W, y)], fill=tuple(int(top[k] + (bottom[k] - top[k]) * t**1.3) for k in range(3)))
    sx, sy = random.randint(450, 1470), random.randint(300, 480)
    g = Image.new("RGB", (W, H))
    ImageDraw.Draw(g).ellipse([sx - 240, sy - 240, sx + 240, sy + 240], fill=sun)
    img = ImageChops.screen(img, g.filter(ImageFilter.GaussianBlur(90)))
    d = ImageDraw.Draw(img)
    d.ellipse([sx - 105, sy - 105, sx + 105, sy + 105], fill=sun)
    for layer in range(4):
        base = H * 0.55 + layer * 80
        f = 0.55 + layer * 0.15
        col = tuple(int(mount[k] * (1.4 - layer * 0.25) + top[k] * 0.1) for k in range(3))
        pts = [(0, H)]
        x = 0
        while x <= W:
            pts.append((x, base - abs(math.sin(x / (210 + layer * 60) + i + layer)) * (180 - layer * 30) - random.randint(0, 45)))
            x += 60
        pts.append((W, H))
        d.polygon(pts, fill=tuple(max(0, min(255, int(c * f))) for c in col))
    return img


def hud(img, i):
    random.seed(100 + i)
    o = Image.new("RGBA", img.size, (0, 0, 0, 0))
    d = ImageDraw.Draw(o)
    # Crosshair.
    cx, cy = W // 2, H // 2
    for dx, dy in [(-22, 0), (12, 0), (0, -22), (0, 12)]:
        d.rectangle([cx + dx, cy + dy, cx + dx + (10 if dy == 0 else 2), cy + dy + (10 if dx == 0 else 2)], fill=(255, 255, 255, 230))
    # Health + shield (bottom left).
    d.rounded_rectangle([60, 960, 520, 1000], 8, fill=(0, 0, 0, 120))
    hp = 0.35 + 0.6 * random.random()
    d.rounded_rectangle([66, 966, 66 + int(448 * hp), 994], 6, fill=(120, 230, 140, 235))
    d.rounded_rectangle([60, 924, 400, 950], 6, fill=(0, 0, 0, 120))
    d.rounded_rectangle([64, 928, 64 + int(332 * random.random()), 946], 5, fill=(110, 180, 255, 235))
    # Ammo (bottom right).
    d.text((1700, 935), f"{random.randint(5, 30)}", font=F_BIG, fill=(255, 255, 255, 240), anchor="ra")
    d.text((1720, 962), f"/ {random.randint(60, 180)}", font=F_HUD, fill=(255, 255, 255, 170))
    # Minimap (top right).
    d.rounded_rectangle([1620, 50, 1860, 290], 16, fill=(0, 0, 0, 110), outline=(255, 255, 255, 90), width=2)
    for _ in range(14):
        w, h = random.randint(10, 40), random.randint(8, 30)
        x, y = random.randint(1640, 1840 - w), random.randint(70, 270 - h)
        d.rectangle([x, y, x + w, y + h], fill=(255, 255, 255, 40))
    d.polygon([(1740, 160), (1730, 182), (1750, 182)], fill=(255, 220, 90, 255))
    # Kill feed with player names (top right, under the map).
    for k in range(3):
        a, b = random.sample(NAMES, 2)
        y = 320 + k * 46
        text = f"{a}  ×  {b}"
        w = d.textlength(text, font=F_SM)
        d.rounded_rectangle([1860 - w - 28, y, 1860, y + 38], 6, fill=(0, 0, 0, 130))
        d.text((1860 - 14, y + 6), text, font=F_SM, fill=(255, 235, 200, 240), anchor="ra")
    # Team chat (bottom left, above health).
    d.rounded_rectangle([60, 640, 640, 900], 10, fill=(0, 0, 0, 110))
    for k, (who, msg) in enumerate(random.sample(CHAT, 4)):
        y = 660 + k * 58
        d.text((80, y), who + ":", font=F_SM, fill=(140, 200, 255, 245))
        d.text((80 + d.textlength(who + ": ", font=F_SM), y), msg, font=F_CHAT, fill=(240, 240, 240, 235))
    # Own name tag (top left).
    d.rounded_rectangle([60, 50, 420, 104], 10, fill=(0, 0, 0, 110))
    d.text((80, 60), "You · " + NAMES[i % len(NAMES)], font=F_HUD, fill=(255, 255, 255, 235))
    return Image.alpha_composite(img.convert("RGBA"), o).convert("RGB")


for i in range(len(pal)):
    base = landscape(i)
    full = hud(base, i)
    full.save(os.path.join(out, f"g{i}.png"))
    base.save(os.path.join(out, f"bg{i}.png"))
    full.resize((640, 360), Image.LANCZOS).save(os.path.join(out, f"t{i}.png"))
    if mock:
        full.resize((640, 360), Image.LANCZOS).save(os.path.join(mock, f"t{i}.png"))
        full.resize((1280, 720), Image.LANCZOS).save(os.path.join(mock, f"f{i}.png"))
print("ok")
