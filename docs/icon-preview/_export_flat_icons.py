"""Export flat high-sat mic icons — capsule + U cradle, edge-to-edge."""
from __future__ import annotations

import math
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[2]
ICONS = ROOT / "src-tauri" / "icons"
PREVIEW = ROOT / "docs" / "icon-preview"
SVG_DIR = PREVIEW / "svg"

COLOR_INIT = (230, 126, 0, 255)  # #E67E00 deep amber
COLOR_READY = (0, 186, 94, 255)  # #00BA5E
COLOR_ERROR = (232, 48, 48, 255)  # #E83030
COLOR_WHITE = (255, 255, 255, 255)


def _pt(cx: float, cy: float, r: float, deg: float) -> tuple[float, float]:
    """Screen coords: 0°=east, CCW, y grows down → use +sin."""
    a = math.radians(deg)
    return cx + r * math.cos(a), cy + r * math.sin(a)


def draw_u(
    d: ImageDraw.ImageDraw,
    cx: float,
    cy: float,
    r_out: float,
    thickness: float,
    a0: float,
    a1: float,
    color: tuple[int, int, int, int],
) -> None:
    """Filled annular sector with round caps (classic mic cradle)."""
    r_in = max(1.0, r_out - thickness)
    n = 72
    outer: list[tuple[float, float]] = []
    inner: list[tuple[float, float]] = []
    for i in range(n + 1):
        t = a0 + (a1 - a0) * i / n
        outer.append(_pt(cx, cy, r_out, t))
    for i in range(n + 1):
        t = a1 - (a1 - a0) * i / n
        inner.append(_pt(cx, cy, r_in, t))
    d.polygon(outer + inner, fill=color)
    r_cap = thickness / 2
    r_mid = (r_out + r_in) / 2
    for ang in (a0, a1):
        x, y = _pt(cx, cy, r_mid, ang)
        d.ellipse([x - r_cap, y - r_cap, x + r_cap, y + r_cap], fill=color)


def draw_mic_tray(size: int, color: tuple[int, int, int, int], with_bang: bool = False) -> Image.Image:
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    # Capsule — wide/tall but leave a clear gap above the U trough
    cw = size * 0.48
    cx0 = (size - cw) / 2
    cy0 = size * 0.015
    ch = size * 0.58
    d.rounded_rectangle([cx0, cy0, cx0 + cw, cy0 + ch], radius=cw / 2, fill=color)

    # U cradle — tips near left/right edges; clear gap under capsule
    draw_u(
        d,
        cx=size / 2,
        cy=size * 0.40,
        r_out=size * 0.49,
        thickness=size * 0.17,
        a0=30,
        a1=150,
        color=color,
    )

    if with_bang:
        bx = size / 2
        bar_w = size * 0.13
        bar_h = size * 0.28
        bar_y0 = cy0 + size * 0.09
        d.rounded_rectangle(
            [bx - bar_w / 2, bar_y0, bx + bar_w / 2, bar_y0 + bar_h],
            radius=bar_w / 2,
            fill=COLOR_WHITE,
        )
        dot_r = size * 0.06
        dot_y = bar_y0 + bar_h + size * 0.07
        d.ellipse([bx - dot_r, dot_y - dot_r, bx + dot_r, dot_y + dot_r], fill=COLOR_WHITE)

    return img


def draw_app_icon(size: int) -> Image.Image:
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle([0, 0, size - 1, size - 1], radius=size * 0.22, fill=COLOR_READY)

    cw = size * 0.40
    cx0 = (size - cw) / 2
    cy0 = size * 0.06
    ch = size * 0.50
    d.rounded_rectangle([cx0, cy0, cx0 + cw, cy0 + ch], radius=cw / 2, fill=COLOR_WHITE)

    draw_u(
        d,
        cx=size / 2,
        cy=size * 0.42,
        r_out=size * 0.42,
        thickness=size * 0.14,
        a0=35,
        a1=145,
        color=COLOR_WHITE,
    )
    return img


def save_png(img: Image.Image, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    img.save(path, "PNG")
    print(f"  wrote {path.relative_to(ROOT)} ({img.size[0]}x{img.size[1]})")


def make_ico(src: Image.Image, path: Path) -> None:
    sizes = [16, 24, 32, 48, 64, 128, 256]
    imgs = [src.resize((s, s), Image.Resampling.LANCZOS) for s in sizes]
    imgs[0].save(path, format="ICO", sizes=[(s, s) for s in sizes])
    print(f"  wrote {path.relative_to(ROOT)} ico")


def write_svgs() -> None:
    SVG_DIR.mkdir(parents=True, exist_ok=True)
    # Exact vector match of geometry (viewBox 0..100)
    tray = """\
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
  <g fill="{color}">
    <rect x="29" y="2" width="42" height="62" rx="21"/>
    <path d="
      M 22.3 33.5
      A 9 9 0 0 0 28.6 42.4
      A 39 39 0 0 0 71.4 42.4
      A 9 9 0 0 0 77.7 33.5
      A 9 9 0 0 0 68.7 27.5
      A 21 21 0 0 1 31.3 27.5
      A 9 9 0 0 0 22.3 33.5
      Z"/>
  </g>
  {bang}
</svg>
"""
    # Simpler U path using arc commands matching draw_u (cx=50,cy=40,rout=48,rin=30,a=38..142)
    # Using stroke-based U is clearer in SVG:
    tray2 = """\
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
  <rect x="26" y="1.5" width="48" height="58" rx="24" fill="{color}"/>
  <path d="M 19.5 28.5 A 49 49 0 0 0 80.5 28.5"
        fill="none" stroke="{color}" stroke-width="17" stroke-linecap="round"/>
  {bang}
</svg>
"""
    bang = """\
  <g fill="#fff">
    <rect x="43.5" y="10" width="13" height="28" rx="6.5"/>
    <circle cx="50" cy="48" r="6"/>
  </g>
"""
    app = """\
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
  <rect width="100" height="100" rx="22" fill="{color}"/>
  <rect x="30" y="6" width="40" height="50" rx="20" fill="#fff"/>
  <path d="M 26.5 32 A 42 42 0 0 0 73.5 32"
        fill="none" stroke="#fff" stroke-width="14" stroke-linecap="round"/>
</svg>
"""
    (SVG_DIR / "mic-tray-init.svg").write_text(tray2.format(color="#E67E00", bang=""), encoding="utf-8")
    (SVG_DIR / "mic-tray-ready.svg").write_text(tray2.format(color="#00BA5E", bang=""), encoding="utf-8")
    (SVG_DIR / "mic-tray-error.svg").write_text(tray2.format(color="#E83030", bang=bang), encoding="utf-8")
    (SVG_DIR / "mic-app.svg").write_text(app.format(color="#00BA5E"), encoding="utf-8")
    # keep unused tray for reference
    _ = tray
    print("  wrote svg masters")


def main() -> None:
    ICONS.mkdir(parents=True, exist_ok=True)
    write_svgs()

    tray_init = draw_mic_tray(512, COLOR_INIT)
    tray_ready = draw_mic_tray(512, COLOR_READY)
    tray_error = draw_mic_tray(512, COLOR_ERROR, with_bang=True)
    app = draw_app_icon(512)

    def preview_tray(src: Image.Image, name: str) -> None:
        bg = Image.new("RGBA", (512, 512), (20, 20, 24, 255))
        bg.alpha_composite(src)
        save_png(bg, PREVIEW / name)

    preview_tray(tray_init, "flat_tray_init.png")
    preview_tray(tray_ready, "flat_tray_ready.png")
    preview_tray(tray_error, "flat_tray_error.png")
    save_png(app, PREVIEW / "flat_app.png")
    save_png(tray_init, PREVIEW / "flat_tray_init_transparent.png")
    save_png(tray_ready, PREVIEW / "flat_tray_ready_transparent.png")
    save_png(tray_error, PREVIEW / "flat_tray_error_transparent.png")

    save_png(app.resize((32, 32), Image.Resampling.LANCZOS), ICONS / "32x32.png")
    save_png(app.resize((128, 128), Image.Resampling.LANCZOS), ICONS / "128x128.png")
    save_png(app.resize((256, 256), Image.Resampling.LANCZOS), ICONS / "128x128@2x.png")
    save_png(app, ICONS / "icon.png")
    make_ico(app, ICONS / "icon.ico")
    save_png(app, ICONS / "icon-init.png")
    save_png(app.resize((32, 32), Image.Resampling.LANCZOS), ICONS / "32x32-init.png")
    save_png(app.resize((128, 128), Image.Resampling.LANCZOS), ICONS / "128x128-init.png")

    for color, bang, base in (
        (COLOR_READY, False, "tray-icon"),
        (COLOR_INIT, False, "tray-icon-init"),
        (COLOR_ERROR, True, "tray-icon-error"),
    ):
        master = draw_mic_tray(512, color, with_bang=bang)
        save_png(master.resize((256, 256), Image.Resampling.LANCZOS), ICONS / f"{base}.png")
        # Native 32px draw → sharper tray than downscale
        save_png(draw_mic_tray(32, color, with_bang=bang), ICONS / f"{base}-32.png")

    print("done")


if __name__ == "__main__":
    main()
