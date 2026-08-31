"""Export docs/icon-3 masters into src-tauri/icons at the sizes the app expects.

Important for Windows/Tauri:
- icon.ico first entry must be 32x32 (Tauri codegen embeds entries[0] as the
  runtime window/taskbar icon). A leading 16x16 loses mic detail and looks wrong.
"""
from __future__ import annotations

import io
import struct
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
SRC = ROOT / "docs" / "icon-3"
ICONS = ROOT / "src-tauri" / "icons"


def content_bbox(im: Image.Image, a_thr: int = 8):
    w, h = im.size
    px = im.load()
    minx, miny, maxx, maxy = w, h, -1, -1
    for y in range(h):
        for x in range(w):
            if px[x, y][3] > a_thr:
                minx = min(minx, x)
                miny = min(miny, y)
                maxx = max(maxx, x)
                maxy = max(maxy, y)
    if maxx < 0:
        return None
    return (minx, miny, maxx + 1, maxy + 1)


def square_crop_pad(im: Image.Image, pad_ratio: float = 0.02, out_size: int = 1024) -> Image.Image:
    bb = content_bbox(im)
    if not bb:
        return im.resize((out_size, out_size), Image.Resampling.LANCZOS)
    cropped = im.crop(bb)
    cw, ch = cropped.size
    side = max(cw, ch)
    pad = int(side * pad_ratio)
    canvas_side = side + pad * 2
    canvas = Image.new("RGBA", (canvas_side, canvas_side), (0, 0, 0, 0))
    ox = (canvas_side - cw) // 2
    oy = (canvas_side - ch) // 2
    canvas.paste(cropped, (ox, oy), cropped)
    return canvas.resize((out_size, out_size), Image.Resampling.LANCZOS)


def harden_alpha(im: Image.Image, cut: int = 16) -> Image.Image:
    im = im.copy()
    px = im.load()
    w, h = im.size
    for y in range(h):
        for x in range(w):
            r, g, b, a = px[x, y]
            if a < cut:
                px[x, y] = (0, 0, 0, 0)
    return im


def load_master(name: str, pad_ratio: float = 0.02) -> Image.Image:
    path = SRC / name
    if not path.exists():
        raise FileNotFoundError(path)
    im = Image.open(path).convert("RGBA")
    im = harden_alpha(im, cut=16)
    return square_crop_pad(im, pad_ratio=pad_ratio, out_size=1024)


def save_png(img: Image.Image, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    img.save(path, "PNG")
    print(f"  {path.name} {img.size[0]}x{img.size[1]}")


def png_bytes(im: Image.Image) -> bytes:
    buf = io.BytesIO()
    im.save(buf, format="PNG")
    return buf.getvalue()


def write_ico(path: Path, images: list[Image.Image]) -> None:
    """Write multi-size ICO. Caller controls entry order (first = Tauri window icon)."""
    entries: list[tuple[int, int, int, int]] = []
    blobs: list[bytes] = []
    offset = 6 + 16 * len(images)
    for im in images:
        blob = png_bytes(im)
        w, h = im.size
        entries.append((0 if w >= 256 else w, 0 if h >= 256 else h, len(blob), offset))
        blobs.append(blob)
        offset += len(blob)
    out = io.BytesIO()
    out.write(struct.pack("<HHH", 0, 1, len(images)))
    for w, h, size, off in entries:
        out.write(struct.pack("<BBBBHHII", w, h, 0, 0, 1, 32, size, off))
    for b in blobs:
        out.write(b)
    path.write_bytes(out.getvalue())
    print(f"  {path.name} ico {path.stat().st_size} bytes, first={images[0].size[0]}x{images[0].size[1]}")


def main() -> None:
    app = load_master("主.png", pad_ratio=0.02)
    save_png(app.resize((32, 32), Image.Resampling.LANCZOS), ICONS / "32x32.png")
    save_png(app.resize((128, 128), Image.Resampling.LANCZOS), ICONS / "128x128.png")
    save_png(app.resize((256, 256), Image.Resampling.LANCZOS), ICONS / "128x128@2x.png")
    save_png(app.resize((512, 512), Image.Resampling.LANCZOS), ICONS / "icon.png")

    # Tauri embeds ICO entries[0] as runtime window/taskbar icon — put 32x32 first.
    ico_order = [32, 16, 24, 48, 64, 128, 256]
    write_ico(
        ICONS / "icon.ico",
        [app.resize((s, s), Image.Resampling.LANCZOS) for s in ico_order],
    )

    init = load_master("托盘_黄色初始化.png", pad_ratio=0.02)
    save_png(init.resize((512, 512), Image.Resampling.LANCZOS), ICONS / "icon-init.png")
    save_png(init.resize((32, 32), Image.Resampling.LANCZOS), ICONS / "32x32-init.png")
    save_png(init.resize((128, 128), Image.Resampling.LANCZOS), ICONS / "128x128-init.png")

    trays = [
        ("托盘_蓝色正常运行.png", "tray-icon"),
        ("托盘_黄色初始化.png", "tray-icon-init"),
        ("拖盘_红色警告.png", "tray-icon-error"),
    ]
    for src_name, base in trays:
        master = load_master(src_name, pad_ratio=0.02)
        save_png(master.resize((256, 256), Image.Resampling.LANCZOS), ICONS / f"{base}.png")
        save_png(master.resize((32, 32), Image.Resampling.LANCZOS), ICONS / f"{base}-32.png")

    print("done")


if __name__ == "__main__":
    main()
