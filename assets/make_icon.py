#!/usr/bin/env python3
"""Draws rterm's app icon (iTerm2-style: dark rounded square, green `$` prompt and cursor).

Usage: python3 -I assets/make_icon.py [output-dir]
Writes rterm.png (512px) and rterm-<size>.png for the hicolor icon sizes. Needs Pillow.
"""
import subprocess
import sys
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageFont

SIZE = 1024
SIZES = [16, 24, 32, 48, 64, 128, 256, 512]


def font(pattern: str, size: int) -> ImageFont.FreeTypeFont:
    path = subprocess.run(["fc-match", "-f", "%{file}", pattern], capture_output=True, text=True).stdout
    return ImageFont.truetype(path, size)


def squircle_mask(size: int, inset: int, n: float = 5.0) -> Image.Image:
    """Superellipse mask, like macOS app icon shapes."""
    mask = Image.new("L", (size, size), 0)
    r = size / 2 - inset
    c = size / 2
    points = []
    steps = 720
    for i in range(steps):
        import math

        t = 2 * math.pi * i / steps
        ct, st = math.cos(t), math.sin(t)
        x = c + r * (abs(ct) ** (2 / n)) * (1 if ct >= 0 else -1)
        y = c + r * (abs(st) ** (2 / n)) * (1 if st >= 0 else -1)
        points.append((x, y))
    ImageDraw.Draw(mask).polygon(points, fill=255)
    return mask


def vertical_gradient(size: int, top: tuple, bottom: tuple) -> Image.Image:
    img = Image.new("RGBA", (size, size))
    draw = ImageDraw.Draw(img)
    for y in range(size):
        t = y / (size - 1)
        color = tuple(round(a + (b - a) * t) for a, b in zip(top, bottom)) + (255,)
        draw.line([(0, y), (size, y)], fill=color)
    return img


def make() -> Image.Image:
    inset = 70
    shape = squircle_mask(SIZE, inset)

    # Soft drop shadow under the tile.
    shadow = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    shadow_mask = squircle_mask(SIZE, inset).filter(ImageFilter.GaussianBlur(18))
    shadow.paste((0, 0, 0, 150), (0, 14), shadow_mask)

    # Near-black screen with a faint violet glow towards the bottom.
    tile = vertical_gradient(SIZE, (6, 6, 10), (26, 12, 34))
    tile.putalpha(shape)

    # Thin light rim.
    rim = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    edge = ImageChops.subtract(shape, squircle_mask(SIZE, inset + 6))
    rim.paste((120, 120, 140, 110), (0, 0), edge)

    # Green `$` prompt and a block cursor, with a phosphor glow.
    glyphs = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    d = ImageDraw.Draw(glyphs)
    green = (95, 214, 95, 255)
    dollar_font = font("DejaVu Sans:bold", 400)
    d.text((150, 150), "$", font=dollar_font, fill=green)
    d.rounded_rectangle((455, 190, 517, 530), radius=8, fill=(70, 180, 70, 255), outline=(40, 120, 40, 255), width=6)
    glow = glyphs.filter(ImageFilter.GaussianBlur(14))
    glow_alpha = glow.getchannel("A").point(lambda a: int(a * 0.7))
    glow.putalpha(glow_alpha)

    out = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    out.alpha_composite(shadow)
    out.alpha_composite(tile)
    content = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    content.alpha_composite(glow)
    content.alpha_composite(glyphs)
    content.putalpha(ImageChops.multiply(content.getchannel("A"), shape))
    out.alpha_composite(content)
    out.alpha_composite(rim)
    return out


def main() -> None:
    out_dir = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).parent
    out_dir.mkdir(parents=True, exist_ok=True)
    icon = make()
    icon.resize((512, 512), Image.LANCZOS).save(out_dir / "rterm.png")
    for s in SIZES:
        icon.resize((s, s), Image.LANCZOS).save(out_dir / f"rterm-{s}.png")
    print(f"wrote icons to {out_dir}")


if __name__ == "__main__":
    main()
