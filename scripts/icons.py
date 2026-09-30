#!/usr/bin/env python3
"""Mirza's icons: the SVG sources, and a render step (Chrome for drawing,
ImageMagick for scaling) that writes every file under assets/icons.

    python3 scripts/icons.py svg      # just the SVGs
    python3 scripts/icons.py render   # SVGs plus PNG, ICO and tray images
    python3 scripts/icons.py sheet out.html   # a preview page
"""
import os, subprocess, sys, tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ICONS = os.path.join(ROOT, "assets", "icons")

BG = ("#2dd4bf", "#0f766e")  # teal, top-left to bottom-right

def mic(color="#fff", ox=0, oy=0, s=1.0):
    """A studio microphone on a 256 grid: capsule with grille slots, cradle, stand."""
    t = lambda x, y: f"{ox + x * s:.1f} {oy + y * s:.1f}"
    w = 17 * s
    slots = "".join(
        f'<path d="M{t(111, y)} L{t(145, y)}" stroke="{BG[1]}" stroke-width="{7 * s:.1f}" stroke-linecap="round" opacity=".55"/>'
        for y in (78, 98, 118)
    ) if color == "#fff" else ""
    return (
        f'<rect x="{ox + 93 * s:.1f}" y="{oy + 34 * s:.1f}" width="{70 * s:.1f}" height="{118 * s:.1f}" rx="{35 * s:.1f}" fill="{color}"/>'
        + slots
        + f'<path d="M{t(64, 116)} A{64 * s:.1f} {64 * s:.1f} 0 0 0 {t(192, 116)}" fill="none" stroke="{color}" stroke-width="{w:.1f}" stroke-linecap="round"/>'
        + f'<path d="M{t(128, 182)} L{t(128, 212)}" stroke="{color}" stroke-width="{w:.1f}" stroke-linecap="round"/>'
        + f'<path d="M{t(94, 214)} L{t(162, 214)}" stroke="{color}" stroke-width="{w:.1f}" stroke-linecap="round"/>'
    )

def tile(inner):
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256" viewBox="0 0 256 256">'
        f'<defs><linearGradient id="bg" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="{BG[0]}"/>'
        f'<stop offset="1" stop-color="{BG[1]}"/></linearGradient></defs>'
        '<rect x="8" y="8" width="240" height="240" rx="60" fill="url(#bg)"/>'
        f"{inner}</svg>\n"
    )

def badge(color, r, mark=""):
    """A dot in the top-right corner with a white ring."""
    return f'<circle cx="206" cy="50" r="{r + 9}" fill="#fff"/><circle cx="206" cy="50" r="{r}" fill="{color}"/>{mark}'

REC_RADII = [24, 28, 32, 36, 40, 44]  # recording dot, quiet to loud
ERROR_MARK = '<rect x="200" y="30" width="12" height="26" rx="5" fill="#fff"/><circle cx="206" cy="68" r="6.5" fill="#fff"/>'

def tray_states():
    """Tray pictures: idle, busy, error, and recording frames rec0..rec5."""
    out = {"idle": tile(mic()), "busy": tile(mic() + badge("#f59e0b", 34)), "error": tile(mic() + badge("#e11d48", 38, ERROR_MARK))}
    for i, r in enumerate(REC_RADII):
        out[f"rec{i}"] = tile(mic() + badge("#e11d48", r))
    return out

def mac_states():
    """macOS menu bar template images: black glyph, the system recolours it."""
    glyph = mic("#000", ox=-14, oy=6, s=0.98)
    def svg(extra=""):
        return f'<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256" viewBox="0 0 256 256">{glyph}{extra}</svg>\n'
    out = {"idle": svg(), "busy": svg("".join(f'<circle cx="{x}" cy="40" r="13" fill="#000"/>' for x in (176, 208, 240))),
           "error": svg('<path d="M214 14 L252 84 L176 84 Z" fill="#000"/>')}
    for i, r in enumerate(REC_RADII):
        out[f"rec{i}"] = svg(f'<circle cx="214" cy="48" r="{r * 0.85:.1f}" fill="#000"/>')
    return out

def write_svgs():
    open(os.path.join(ICONS, "io.github.erfnemati.Mirza.svg"), "w").write(tile(mic()))

def chrome_png(svg_text, path, size=1024):
    with tempfile.TemporaryDirectory() as d:
        svg = os.path.join(d, "i.svg"); open(svg, "w").write(svg_text)
        html = os.path.join(d, "i.html")
        open(html, "w").write(f'<html><body style="margin:0;background:transparent"><img src="file://{svg}" width="{size}" height="{size}" style="display:block"></body></html>')
        subprocess.run(["google-chrome", "--headless=new", "--disable-gpu", "--no-sandbox", "--hide-scrollbars",
                        "--default-background-color=00000000", f"--window-size={size},{size}", f"--screenshot={path}", f"file://{html}"],
                       check=True, capture_output=True)

def scale(src, dst, size, fmt=""):
    subprocess.run(["magick", src, "-filter", "Lanczos", "-resize", f"{size}x{size}", "-depth", "8", f"{fmt}{dst}"], check=True)

def render():
    write_svgs()
    png = os.path.join(ICONS, "png"); tray = os.path.join(ICONS, "tray")
    os.makedirs(png, exist_ok=True); os.makedirs(tray, exist_ok=True)
    with tempfile.TemporaryDirectory() as d:
        big = os.path.join(d, "app.png")
        chrome_png(tile(mic()), big)
        for s in (16, 32, 64, 128, 256, 512):
            scale(big, os.path.join(png, f"mirza-{s}.png"), s)
        subprocess.run(["cp", big, os.path.join(png, "mirza-1024.png")], check=True)
        subprocess.run(["magick", *[os.path.join(png, f"mirza-{s}.png") for s in (16, 32, 64, 128, 256)], os.path.join(ICONS, "mirza.ico")], check=True)
        for name, svg in tray_states().items():
            p = os.path.join(d, f"t-{name}.png"); chrome_png(svg, p)
            for s in (22, 32, 44):
                scale(p, os.path.join(tray, f"{name}-{s}.rgba"), s, "rgba:")
        for name, svg in mac_states().items():
            p = os.path.join(d, f"m-{name}.png"); chrome_png(svg, p)
            scale(p, os.path.join(tray, f"mac-{name}-36.rgba"), 36, "rgba:")
    panel = os.path.join(ROOT, "crates", "panel")
    for s, name in ((32, "32x32.png"), (128, "128x128.png"), (512, "icon.png")):
        subprocess.run(["cp", os.path.join(png, f"mirza-{s}.png"), os.path.join(panel, "icons", name)], check=True)
    subprocess.run(["cp", os.path.join(ICONS, "io.github.erfnemati.Mirza.svg"), os.path.join(panel, "ui", "logo.svg")], check=True)

def sheet(out):
    enc = lambda s: s.replace("#", "%23").replace("\n", "")
    img = lambda svg, sz, bg="none": f"<img src='data:image/svg+xml;utf8,{enc(svg)}' width={sz} height={sz} style='background:{bg}'>"
    states = tray_states(); macs = mac_states()
    rows = ""
    for bg in ("#e8e8ea", "#1f2023"):
        rows += f"<div style='display:flex;gap:14px;align-items:end;padding:12px;background:{bg}'>"
        rows += "".join(img(states[k], sz) for k in ("idle", "rec0", "rec3", "rec5", "busy", "error") for sz in (22,))
        rows += "<span style='width:20px'></span>" + "".join(img(states[k], 44) for k in ("idle", "rec0", "rec5", "busy", "error"))
        rows += "<span style='width:20px'></span>" + "".join(img(macs[k], 22) for k in ("idle", "rec0", "rec5", "busy", "error"))
        rows += "</div>"
    open(out, "w").write(f"<html><body style='margin:0;padding:16px;background:#f4f4f5'>{rows}<div style='padding:16px'>{img(tile(mic()), 200)} {img(tile(mic()), 64)} {img(tile(mic()), 32)} {img(tile(mic()), 16)}</div></body></html>")

if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else "svg"
    {"svg": write_svgs, "render": render}.get(cmd, lambda: sheet(sys.argv[2]))()
