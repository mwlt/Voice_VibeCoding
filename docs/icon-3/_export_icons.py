"""Export docs/icon-3 masters into src-tauri/icons at the sizes the app expects."""
from __future__ import annotations

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


def square_crop_pad(im: Image.Image, pad_ratio: float = 0.04, out_size: int = 1024) -> Image.Image:
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


def harden_alpha(im: Image.Image, cut: int = 20) -> Image.Image:
    im = im.copy()
    px = im.load()
    w, h = im.size
    for y in range(h):
        for x in range(w):
            r, g, b, a = px[x, y]
            if a < cut:
                px[x, y] = (0, 0, 0, 0)
    return im


def load_master(name: str, pad_ratio: float = 0.04) -> Image.Image:
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


def make_ico(src: Image.Image, path: Path) -> None:
    sizes = [(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]
    # PNG-compressed ICO entries keep alpha (BMP path often collapses to tiny/broken files)
    src.save(path, format="ICO", sizes=sizes, bitmap_format="png")
    print(f"  {path.name} ico {path.stat().st_size} bytes")


def main() -> None:
    # 主图 → 窗口/任务栏/安装包图标
    app = load_master("主.png", pad_ratio=0.03)
    app512 = app.resize((512, 512), Image.Resampling.LANCZOS)
    save_png(app.resize((32, 32), Image.Resampling.LANCZOS), ICONS / "32x32.png")
    save_png(app.resize((128, 128), Image.Resampling.LANCZOS), ICONS / "128x128.png")
    save_png(app.resize((256, 256), Image.Resampling.LANCZOS), ICONS / "128x128@2x.png")
    save_png(app512, ICONS / "icon.png")
    make_ico(app, ICONS / "icon.ico")

    # 初始化态命名资源（窗口仍用主图；保留 init 变体供备用）
    init = load_master("托盘_黄色初始化.png", pad_ratio=0.03)
    init512 = init.resize((512, 512), Image.Resampling.LANCZOS)
    save_png(init512, ICONS / "icon-init.png")
    save_png(init.resize((32, 32), Image.Resampling.LANCZOS), ICONS / "32x32-init.png")
    save_png(init.resize((128, 128), Image.Resampling.LANCZOS), ICONS / "128x128-init.png")

    # 托盘三态（文件名「拖盘」为源文件拼写）
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
