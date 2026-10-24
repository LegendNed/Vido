# Contact sheet of rendered stills, for reviewing the trailer quickly.
# usage: python sheet.py <stills_dir> <out.png>
import glob, sys
from PIL import Image, ImageDraw
files = sorted(glob.glob(sys.argv[1] + "/t*.png"))
cols = 4; w, h = 640, 360
rows = (len(files) + cols - 1) // cols
sheet = Image.new("RGB", (cols * w, rows * h), "black")
d = ImageDraw.Draw(sheet)
for i, f in enumerate(files):
    im = Image.open(f).convert("RGB").resize((w, h))
    x, y = (i % cols) * w, (i // cols) * h
    sheet.paste(im, (x, y))
    d.text((x + 8, y + 6), f.split("t")[-1][:-4].lstrip("0") or "0", fill="yellow")
sheet.save(sys.argv[2])
