"""Renders assets/kiln.ico (Windows app icon) from the same geometry as ui/logo.slint.

Run: python crates/kiln-shell/assets/make_icon.py   (needs Pillow)
"""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFilter

HERE = Path(__file__).parent
S = 1024  # draw large, downsample for smooth edges
BG = (11, 15, 20, 255)
OUTLINE = (115, 125, 140, 255)
ACCENT = (249, 115, 22)
HOT = (253, 196, 156, 255)


def pts(path, inset=0.12):
    # Map the 100x100 design grid into the icon with a margin.
    k = S * (1 - 2 * inset) / 100
    o = S * inset
    return [(o + x * k, o + y * k) for x, y in path]


STEM = [(12, 8), (32, 8), (32, 92), (12, 92), (12, 8)]
ARMS = [(66, 8), (92, 8), (78, 27.6), (78, 72.4), (92, 92), (66, 92), (36, 50), (66, 8)]
CHEVRON = [(66, 22), (46, 50), (66, 78)]


def render():
    k = S * 0.76 / 100
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle([0, 0, S - 1, S - 1], radius=S * 0.2, fill=BG)

    # Glow: blurred thick chevron under the line.
    glow = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    ImageDraw.Draw(glow).line(pts(CHEVRON), fill=ACCENT + (170,), width=int(10 * k), joint="curve")
    img.alpha_composite(glow.filter(ImageFilter.GaussianBlur(S * 0.04)))

    for p in (STEM, ARMS):
        d.line(pts(p), fill=OUTLINE, width=int(4 * k), joint="curve")
    d.line(pts(CHEVRON), fill=ACCENT + (255,), width=int(3.4 * k), joint="curve")
    d.line(pts(CHEVRON), fill=HOT, width=int(1.2 * k), joint="curve")
    return img


if __name__ == "__main__":
    big = render()
    big.resize((256, 256), Image.LANCZOS).save(
        HERE / "kiln.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]
    )
    big.resize((512, 512), Image.LANCZOS).save(HERE / "kiln-512.png")
    print("wrote kiln.ico, kiln-512.png")
