"""Draws the memwarden icon: a blue shield with three memory bars on a dark tile.

Writes assets/memwarden.ico (16-256 px), assets/icon-256.png and the browser
extension's extension/icons/icon-<size>.png. Needed only when the icon
changes; the outputs are committed. Requires Pillow.

    python scripts/make-icon.py
"""

from pathlib import Path

from PIL import Image, ImageDraw

# Colors from src/theme.rs.
TILE = (0x18, 0x1E, 0x28, 255)
BORDER = (0x26, 0x2E, 0x3B, 255)
BLUE = (0x4C, 0x9A, 0xFF, 255)
BG = (0x0F, 0x13, 0x1A, 255)

SIZES = [16, 24, 32, 48, 64, 128, 256]
EXTENSION_SIZES = [16, 32, 48, 128]
SUPERSAMPLE = 4


def bezier(p0, p1, p2, steps=24):
    """Points of a quadratic Bezier curve from p0 to p2."""
    out = []
    for i in range(steps + 1):
        t = i / steps
        x = (1 - t) ** 2 * p0[0] + 2 * (1 - t) * t * p1[0] + t**2 * p2[0]
        y = (1 - t) ** 2 * p0[1] + 2 * (1 - t) * t * p1[1] + t**2 * p2[1]
        out.append((x, y))
    return out


def shield(s, inset=0.0):
    """Shield outline in a unit square scaled to `s`, shrunk by `inset`."""
    left, right, top, waist, tip = 0.20 + inset, 0.80 - inset, 0.16 + inset, 0.50, 0.88 - inset * 1.4
    pts = [(left, top), (0.5, top - 0.035), (right, top), (right, waist)]
    pts += bezier((right, waist), (right, 0.74), (0.5, tip))[1:]
    pts += bezier((0.5, tip), (left, 0.74), (left, waist))[1:]
    return [(x * s, y * s) for x, y in pts]


def draw(size):
    """The icon at `size` px. Small sizes drop the tile border and use wider bars."""
    s = size * SUPERSAMPLE
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    border = 0 if size < 32 else max(1, s // 64)
    d.rounded_rectangle([0, 0, s - 1, s - 1], radius=s * 0.22, fill=TILE, outline=BORDER, width=border)
    d.polygon(shield(s), fill=BLUE)
    # Memory bars of rising height, cut out of the shield.
    bars = [(0.30, 0.42), (0.45, 0.57), (0.60, 0.72)] if size < 32 else [(0.32, 0.42), (0.45, 0.55), (0.58, 0.68)]
    heights = [0.14, 0.22, 0.30]
    bottom = 0.66
    for (x0, x1), h in zip(bars, heights):
        d.rounded_rectangle([x0 * s, (bottom - h) * s, x1 * s, bottom * s], radius=s * 0.012, fill=BG)
    return img.resize((size, size), Image.LANCZOS)


def main():
    root = Path(__file__).resolve().parent.parent
    out = root / "assets"
    out.mkdir(exist_ok=True)
    images = [draw(size) for size in SIZES]
    images[-1].save(out / "icon-256.png")
    images[-1].save(out / "memwarden.ico", sizes=[(s, s) for s in SIZES], append_images=images[:-1])
    icons = root / "extension" / "icons"
    icons.mkdir(exist_ok=True)
    for size in EXTENSION_SIZES:
        images[SIZES.index(size)].save(icons / f"icon-{size}.png")
    # Preview of every size side by side, for checking legibility.
    preview = Image.new("RGBA", (sum(SIZES) + 10 * len(SIZES), 256), (255, 255, 255, 255))
    x = 0
    for img in images:
        preview.paste(img, (x, 256 - img.height), img)
        x += img.width + 10
    return preview


if __name__ == "__main__":
    import sys

    preview = main()
    if len(sys.argv) > 1:
        preview.save(sys.argv[1])
