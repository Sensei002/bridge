#!/usr/bin/env python3
"""Generate every platform icon from the original Bridge master artwork."""
from pathlib import Path
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "assets" / "Bridge — Connect, Share, Beyond.png"
if not SOURCE.is_file():
    raise SystemExit(f"Missing master Bridge artwork: {SOURCE}")

brand = Image.open(SOURCE).convert("RGBA")
# Keep the full wordmark/tagline intact for in-app and README branding.
brand.save(ROOT / "apps/ios/BridgeApp/Assets.xcassets/BridgeBrand.imageset/BridgeBrand.png", "PNG", optimize=True)

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
app_icon(1024).save(ROOT / "apps/ios/BridgeApp/Assets.xcassets/AppIcon.appiconset/AppIcon.png", "PNG", optimize=True)

# Android legacy launcher icon resources (all standard density buckets).
android_res = ROOT / "apps/android/app/src/main/res"
for density, size in {"mdpi":48, "hdpi":72, "xhdpi":96, "xxhdpi":144, "xxxhdpi":192}.items():
    folder = android_res / f"mipmap-{density}"
    folder.mkdir(parents=True, exist_ok=True)
    app_icon(size).save(folder / "ic_launcher.png", "PNG", optimize=True)
    app_icon(size).save(folder / "ic_launcher_round.png", "PNG", optimize=True)
