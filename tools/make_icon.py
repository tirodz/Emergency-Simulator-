from pathlib import Path
from PIL import Image, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "src-tauri" / "icons"
OUT.mkdir(parents=True, exist_ok=True)
S = 1024

img = Image.new("RGBA", (S, S), (0, 0, 0, 0))

# Minimal, modern mark: dark glass tile + white E + emerald broadcast symbol.
glow = Image.new("RGBA", (S, S), (0, 0, 0, 0))
g = ImageDraw.Draw(glow)
g.rounded_rectangle((98, 98, 926, 926), radius=185, fill=(85, 241, 160, 52))
glow = glow.filter(ImageFilter.GaussianBlur(34))
img = Image.alpha_composite(img, glow)

d = ImageDraw.Draw(img)
d.rounded_rectangle((92, 92, 932, 932), radius=190, fill=(8, 13, 11, 250),
                    outline=(255, 255, 255, 34), width=3)
d.rounded_rectangle((104, 104, 920, 920), radius=176, outline=(82, 241, 160, 125), width=5)

# Soft glass reflection, kept subtle so the symbol remains legible at 16px.
ref = Image.new("RGBA", (S, S), (0, 0, 0, 0))
rd = ImageDraw.Draw(ref)
rd.polygon([(112, 320), (330, 112), (920, 112), (920, 230), (390, 230), (160, 460)],
           fill=(255, 255, 255, 15))
img = Image.alpha_composite(img, ref)

d = ImageDraw.Draw(img)
white = (244, 248, 246, 255)
green = (82, 241, 160, 255)

# Geometric E, deliberately simple and recognizable.
d.polygon([
    (258, 258), (704, 258), (748, 302), (356, 302),
    (356, 430), (666, 430), (704, 472), (356, 472),
    (356, 600), (748, 600), (704, 644), (258, 644)
], fill=white)

# Broadcast mark.
d.ellipse((690, 414, 770, 494), fill=green)
d.arc((704, 338, 888, 570), start=300, end=60, fill=green, width=26)
d.arc((704, 286, 948, 622), start=300, end=60, fill=(52, 219, 139, 235), width=19)

# Crisp inner highlight.
d.rounded_rectangle((107, 107, 917, 917), radius=174, outline=(255, 255, 255, 26), width=2)

img.save(OUT / "icon.png", optimize=True)
img.save(OUT / "icon.ico", sizes=[(256,256), (128,128), (64,64), (48,48), (32,32), (16,16)])
print("Generated", OUT / "icon.png")
print("Generated", OUT / "icon.ico")
