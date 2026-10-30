"""Regenerate committed PNG/ICO from app.svg. Developer tool; requires Rust and Pillow."""
from pathlib import Path
import subprocess
from PIL import Image

root = Path(__file__).resolve().parent.parent
pixels = subprocess.check_output([
    "cargo", "run", "--locked", "--quiet", "--features", "desktop", "--example", "render-icon"
], cwd=root)
if len(pixels) != 256 * 256 * 4:
    raise SystemExit("Unexpected RGBA output length")
image = Image.frombytes("RGBA", (256, 256), pixels)
image.save(root / "assets/app.png")
image.save(root / "assets/app.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
print("Generated assets/app.png and assets/app.ico")
