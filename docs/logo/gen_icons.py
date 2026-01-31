"""Generate the ordin application icon files from the variant A SVGs.

Writes, for the light and the dark variant of the default brand:
  macos/ordin.icns       built with iconutil from an .iconset
  windows/ordin.ico      PNG-compressed entries, 16 to 256 px
  tauri/                 the five files a Tauri bundle lists by default
  png/                   every rendered size, for review

Run from this folder: python3 -I gen_icons.py
Needs rsvg-convert and, for the .icns, macOS iconutil. No network access.
"""
import os, re, shutil, struct, subprocess, sys, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "icons")

# The mark's form follows the size the icon is shown at (see index.html).
def form_for(points):
    if points >= 48:
        return "full"
    if points >= 24:
        return "compact"
    return "small"


MARKS = {}
for form in ("full", "compact", "small"):
    src = open(os.path.join(HERE, f"a-open-orbit-{form}.svg")).read()
    MARKS[form] = re.sub(r"^<svg[^>]*>|</svg>\s*$", "", src.strip())

# Tile and mark colours from the interface concept's tokens.
VARIANTS = {
    "light": {"tile": "#1F4FB8", "mark": "#FFFFFF"},
    "dark": {"tile": "#22314A", "mark": "#A9C3FA"},
}

# Tile geometry as fractions of the canvas.
# macOS: an 824/1024 rounded square centred on the canvas, per Apple's grid.
# Windows: the tile fills the canvas with a small margin.
PLATFORM = {
    "macos": {"inset": 100 / 1024, "radius": 0.224},
    "windows": {"inset": 1 / 32, "radius": 0.18},
}


def icon_svg(px, points, variant, platform):
    g = PLATFORM[platform]
    v = VARIANTS[variant]
    form = form_for(points)
    inset = px * g["inset"]
    side = px - 2 * inset
    scale = 0.8 if form == "small" else 0.66
    mark = side * scale
    off = inset + (side - mark) / 2
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{px}" height="{px}" viewBox="0 0 {px} {px}">'
            f'<rect x="{inset}" y="{inset}" width="{side}" height="{side}" rx="{side*g["radius"]}" fill="{v["tile"]}"/>'
            f'<g transform="translate({off} {off}) scale({mark/64})" fill="{v["mark"]}" color="{v["mark"]}">'
            f'{MARKS[form]}</g></svg>')


def render(svg_text, dest):
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    subprocess.run(["rsvg-convert", "-o", dest], input=svg_text.encode(), check=True)


def write_ico(pngs, dest):
    """ICO container holding PNG images (supported since Windows Vista)."""
    data = [(px, open(p, "rb").read()) for px, p in pngs]
    header = struct.pack("<HHH", 0, 1, len(data))
    offset = 6 + 16 * len(data)
    entries, blobs = b"", b""
    for px, blob in data:
        dim = 0 if px >= 256 else px
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(blob), offset)
        offset += len(blob)
        blobs += blob
    with open(dest, "wb") as fh:
        fh.write(header + entries + blobs)


MAC_SIZES = [(16, 1), (16, 2), (32, 1), (32, 2), (128, 1), (128, 2), (256, 1), (256, 2), (512, 1), (512, 2)]
WIN_SIZES = [16, 20, 24, 32, 40, 48, 64, 256]

if os.path.isdir(OUT):
    shutil.rmtree(OUT)

for variant in VARIANTS:
    base = os.path.join(OUT, variant)
    png = os.path.join(base, "png")

    # macOS
    iconset = os.path.join(tempfile.mkdtemp(), "ordin.iconset")
    os.makedirs(iconset)
    for pt, mult in MAC_SIZES:
        px = pt * mult
        name = f"icon_{pt}x{pt}{'@2x' if mult == 2 else ''}.png"
        render(icon_svg(px, pt, variant, "macos"), os.path.join(iconset, name))
        os.makedirs(png, exist_ok=True)
        shutil.copy(os.path.join(iconset, name), os.path.join(png, f"macos-{name}"))
    os.makedirs(os.path.join(base, "macos"), exist_ok=True)
    if shutil.which("iconutil"):
        subprocess.run(["iconutil", "-c", "icns", iconset, "-o", os.path.join(base, "macos", "ordin.icns")], check=True)
    else:
        print("iconutil not found: skipped ordin.icns", file=sys.stderr)

    # Windows
    win = []
    for px in WIN_SIZES:
        dest = os.path.join(png, f"windows-{px}.png")
        render(icon_svg(px, px, variant, "windows"), dest)
        win.append((px, dest))
    os.makedirs(os.path.join(base, "windows"), exist_ok=True)
    write_ico(win, os.path.join(base, "windows", "ordin.ico"))

    # Tauri's default bundle icon list
    tauri = os.path.join(base, "tauri")
    for px, pt, name in ((32, 32, "32x32.png"), (128, 128, "128x128.png"),
                         (256, 128, "128x128@2x.png"), (512, 512, "icon.png")):
        render(icon_svg(px, pt, variant, "macos"), os.path.join(tauri, name))
    shutil.copy(os.path.join(base, "macos", "ordin.icns"), os.path.join(tauri, "icon.icns"))
    shutil.copy(os.path.join(base, "windows", "ordin.ico"), os.path.join(tauri, "icon.ico"))

for root, _, files in sorted(os.walk(OUT)):
    for name in sorted(files):
        p = os.path.join(root, name)
        print(f"{os.path.relpath(p, HERE)}\t{os.path.getsize(p)}")
