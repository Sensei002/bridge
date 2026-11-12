#!/usr/bin/env python3
"""Generate Windows and Android app icons from the original Bridge master artwork."""
from pathlib import Path
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "assets" / "Bridge — Connect, Share, Beyond.png"
if not SOURCE.is_file():
    raise SystemExit(f"Missing master Bridge artwork: {SOURCE}")

brand = Image.open(SOURCE).convert("RGBA")
# Store/app icons must be square. Fit the complete logo on a dark canvas rather
# than stretching or cropping the original horizontal artwork.
def app_icon(size: int) -> Image.Image:
    canvas = Image.new("RGBA", (size, size), (10, 10, 10, 255))
    margin = max(2, round(size * 0.055))
    available = (size - margin * 2, size - margin * 2)
    fitted = brand.copy()
    fitted.thumbnail(available, Image.Resampling.LANCZOS)
    canvas.alpha_composite(fitted, ((size - fitted.width) // 2, (size - fitted.height) // 2))
    return canvas.convert("RGB")

app_icon(512).save(ROOT / "assets/bridge-app-icon.png", "PNG", optimize=True)

# Android adaptive-icon foreground and in-app header both reference this drawable.
android_drawables = ROOT / "apps/android/app/src/main/res/drawable-nodpi"
android_drawables.mkdir(parents=True, exist_ok=True)
app_icon(512).save(android_drawables / "bridge_app_icon.png", "PNG", optimize=True)

# Android legacy launcher icon resources (all standard density buckets).
android_res = ROOT / "apps/android/app/src/main/res"
for density, size in {"mdpi":48, "hdpi":72, "xhdpi":96, "xxhdpi":144, "xxxhdpi":192}.items():
    folder = android_res / f"mipmap-{density}"
    folder.mkdir(parents=True, exist_ok=True)
    app_icon(size).save(folder / "ic_launcher.png", "PNG", optimize=True)
    app_icon(size).save(folder / "ic_launcher_round.png", "PNG", optimize=True)
