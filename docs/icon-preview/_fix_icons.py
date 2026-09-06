from PIL import Image
from collections import deque
from pathlib import Path

base = Path(r"d:\00vscode_workspace\remote-bridge-hub-master\xiaomi_remote_2_pro_rust_deepseek\docs\icon-preview")


def flood_to_transparent(im: Image.Image, is_bg) -> Image.Image:
    im = im.copy().convert("RGBA")
    w, h = im.size
    px = im.load()
    visited = [[False] * w for _ in range(h)]
    q = deque()
    seeds = [
        (0, 0),
        (w - 1, 0),
        (0, h - 1),
        (w - 1, h - 1),
        (w // 2, 0),
        (w // 2, h - 1),
        (0, h // 2),
        (w - 1, h // 2),
    ]
    for x, y in seeds:
        if is_bg(*px[x, y]):
            visited[y][x] = True
            q.append((x, y))
    while q:
        x, y = q.popleft()
        px[x, y] = (0, 0, 0, 0)
        for nx, ny in ((x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)):
            if 0 <= nx < w and 0 <= ny < h and not visited[ny][nx]:
                if is_bg(*px[nx, ny]):
                    visited[ny][nx] = True
                    q.append((nx, ny))
    return im


def harden_alpha(im: Image.Image, cut=40) -> Image.Image:
    im = im.copy()
    px = im.load()
    w, h = im.size
    for y in range(h):
        for x in range(w):
            r, g, b, a = px[x, y]
            if a < cut:
                px[x, y] = (0, 0, 0, 0)
    return im


def purify_exclamation_white(im: Image.Image) -> Image.Image:
    im = im.copy()
    w, h = im.size
    px = im.load()
    xs, ys = [], []
    for y in range(h):
        for x in range(w):
            r, g, b, a = px[x, y]
            if a > 200 and r > 140 and r > g + 20 and r > b + 20:
                xs.append(x)
                ys.append(y)
    if not xs:
        return im
    for y in range(min(ys), max(ys) + 1):
        for x in range(min(xs), max(xs) + 1):
            r, g, b, a = px[x, y]
            if a == 0:
                continue
            bright = (r + g + b) / 3.0
            if bright >= 200 and abs(r - g) < 25 and abs(g - b) < 25:
                px[x, y] = (255, 255, 255, 255)
            elif a < 180 and bright >= 180:
                px[x, y] = (0, 0, 0, 0)
    return im


def content_bbox(im: Image.Image, a_thr=8):
    w, h = im.size
    px = im.load()
    minx, miny, maxx, maxy = w, h, -1, -1
    for y in range(h):
        for x in range(w):
            if px[x, y][3] > a_thr:
                if x < minx:
                    minx = x
                if y < miny:
                    miny = y
                if x > maxx:
                    maxx = x
                if y > maxy:
                    maxy = y
    if maxx < 0:
        return None
    return (minx, miny, maxx + 1, maxy + 1)


def square_crop_pad(im: Image.Image, pad_ratio=0.08, out_size=1024) -> Image.Image:
    bb = content_bbox(im)
    if not bb:
        return im
    cropped = im.crop(bb)
    cw, ch = cropped.size
    side = max(cw, ch)
    pad = int(side * pad_ratio)
    canvas_side = side + pad * 2
    canvas = Image.new("RGBA", (canvas_side, canvas_side), (0, 0, 0, 0))
    ox = (canvas_side - cw) // 2
    oy = (canvas_side - ch) // 2
    canvas.paste(cropped, (ox, oy), cropped)
    if canvas_side != out_size:
        canvas = canvas.resize((out_size, out_size), Image.Resampling.LANCZOS)
    return canvas


def bg_light_or_dust(r, g, b, a):
    if a == 0:
        return True
    if min(r, g, b) >= 230 and abs(r - g) < 12 and abs(g - b) < 12:
        return True
    if a < 30 and min(r, g, b) >= 200:
        return True
    if a < 200 and min(r, g, b) >= 235:
        return True
    return False


def process_tray(src: Path, dst: Path, fix_bang=False):
    im = Image.open(src).convert("RGBA")
    im = flood_to_transparent(im, bg_light_or_dust)
    im = harden_alpha(im, cut=35)
    if fix_bang:
        im = purify_exclamation_white(im)
        im = flood_to_transparent(im, bg_light_or_dust)
        im = purify_exclamation_white(im)
    im = harden_alpha(im, cut=30)
    im = square_crop_pad(im, pad_ratio=0.10, out_size=1024)
    im.save(dst, "PNG")
    print(dst.name, "bbox", content_bbox(im), "corner", im.getpixel((2, 2)))


def process_app(src: Path, dst: Path):
    im = Image.open(src).convert("RGBA")
    im = flood_to_transparent(im, bg_light_or_dust)
    im = harden_alpha(im, cut=25)
    im = square_crop_pad(im, pad_ratio=0.04, out_size=1024)
    im.save(dst, "PNG")
    print(dst.name, "corner", im.getpixel((2, 2)), "bbox", content_bbox(im))


# Work from current previews (may already be semi-cleaned)
process_tray(base / "01-tray-error-red.png", base / "01-tray-error-red.png", fix_bang=True)
process_tray(base / "02-tray-init-yellow.png", base / "02-tray-init-yellow.png")
process_tray(base / "03-tray-ready-green.png", base / "03-tray-ready-green.png")
process_app(base / "04-app-icon.png", base / "04-app-icon.png")

im = Image.open(base / "01-tray-error-red.png").convert("RGBA")
w, h = im.size
whites = []
for y in range(h):
    for x in range(w):
        r, g, b, a = im.getpixel((x, y))
        if a > 200 and r > 240 and g > 240 and b > 240:
            whites.append((r, g, b, a))
print("red near-white count", len(whites))
if whites:
    print("all pure #fff?", all(p == (255, 255, 255, 255) for p in whites))
    print("sample", whites[0])
