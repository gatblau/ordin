"""Generate the ordin logo, variant A (crew orbiting a document), as local SVGs
plus a self-contained review sheet. No external fetches."""
import math, os, sys

OUT = sys.argv[1]
os.makedirs(OUT, exist_ok=True)

C, R = 32.0, 24.0  # centre and orbit radius on a 64-unit grid


def f(n):
    return f"{n:.2f}".rstrip("0").rstrip(".")


def page(x, y, w, h, fold, gap, rad):
    """Page with a folded top-end corner, drawn as two shapes split by a gap."""
    body = (f"M{f(x+rad)} {f(y)}H{f(x+w-fold-gap)}V{f(y+fold+gap)}H{f(x+w)}"
            f"V{f(y+h-rad)}a{f(rad)} {f(rad)} 0 0 1 {f(-rad)} {f(rad)}"
            f"H{f(x+rad)}a{f(rad)} {f(rad)} 0 0 1 {f(-rad)} {f(-rad)}"
            f"V{f(y+rad)}a{f(rad)} {f(rad)} 0 0 1 {f(rad)} {f(-rad)}Z")
    flap = f"M{f(x+w-fold)} {f(y)}L{f(x+w)} {f(y+fold)}H{f(x+w-fold)}Z"
    return body, flap


def bar(x, y, w, h):
    """Rounded bar, used as a text line cut out of the page (even-odd hole)."""
    r = h / 2
    return (f"M{f(x+r)} {f(y)}H{f(x+w-r)}a{f(r)} {f(r)} 0 0 1 0 {f(h)}"
            f"H{f(x+r)}a{f(r)} {f(r)} 0 0 1 0 {f(-h)}Z")


def pt(deg, r=R):
    a = math.radians(deg)
    return C + r * math.cos(a), C + r * math.sin(a)


def orbit(dots, r=R, gap=2.4):
    """Arcs of the orbit, broken around each crew dot. dots = [(deg, radius)]."""
    cuts = sorted((d, math.degrees((dr + gap) / r)) for d, dr in dots)
    arcs = []
    for i, (d, half) in enumerate(cuts):
        nd, nhalf = cuts[(i + 1) % len(cuts)]
        start, end = d + half, nd - nhalf + (360 if i == len(cuts) - 1 else 0)
        sx, sy = pt(start, r)
        ex, ey = pt(end, r)
        large = 1 if (end - start) > 180 else 0
        arcs.append(f"M{f(sx)} {f(sy)}A{f(r)} {f(r)} 0 {large} 1 {f(ex)} {f(ey)}")
    return "".join(arcs)


def dots_svg(dots, r=R):
    out = []
    for d, dr in dots:
        x, y = pt(d, r)
        out.append(f'<circle cx="{f(x)}" cy="{f(y)}" r="{f(dr)}"/>')
    return "".join(out)


def disc_with_page(r, pg):
    body, flap = pg
    ring = f"M{f(C-r)} {C}a{f(r)} {f(r)} 0 1 0 {f(2*r)} 0a{f(r)} {f(r)} 0 1 0 {f(-2*r)} 0Z"
    return f'<path fill-rule="evenodd" d="{ring}{body}{flap}"/>'


PAGE = page(22, 18, 20, 28, 7, 1.8, 2)
PAGE_SMALL = page(20.5, 16, 23, 32, 8.5, 2.6, 2.5)


LINES = bar(26, 31.4, 12, 2.6) + bar(26, 37.4, 8, 2.6)
CREW = [(-45, 5.0), (150, 3.4)]
RING = f'<path fill="none" stroke="currentColor" stroke-width="4" d="{orbit(CREW)}"/>'

# Variant A in three forms: full (text lines) for 48px and up, compact (no
# lines) for 24-47px, small (no crew) for under 24px.
FORMS = {
    "full": f'{RING}<path fill-rule="evenodd" d="{PAGE[0]}{LINES}"/><path d="{PAGE[1]}"/>{dots_svg(CREW)}',
    "compact": f'{RING}<path d="{PAGE[0]}"/><path d="{PAGE[1]}"/>{dots_svg(CREW)}',
    "small": (f'<circle cx="32" cy="32" r="25" fill="none" stroke="currentColor" stroke-width="6"/>'
              f'<path d="{PAGE_SMALL[0]}"/><path d="{PAGE_SMALL[1]}"/>'),
}


def svg(inner, size=None, cls=""):
    sz = f' width="{size}" height="{size}"' if size else ""
    c = f' class="{cls}"' if cls else ""
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"{sz}{c} '
            f'fill="currentColor" aria-hidden="true">{inner}</svg>')


for form, inner in FORMS.items():
    with open(os.path.join(OUT, f"a-open-orbit-{form}.svg"), "w") as fh:
        fh.write(svg(inner) + "\n")


# Logotype: lowercase "ordin", drawn as outlines on the mark's round geometry.
# Baseline y=56, x-height y=16, ascender y=0, stem weight 7.
W, XH, BASE = 7.0, 16.0, 56.0
MID = (XH + BASE) / 2          # centre of the bowls and arches
BOWL = 20.6                    # outer bowl radius, with overshoot
ARCH = (BASE - XH) / 2         # outer arch radius, flat with the x-height


def ring(cx, cy, ro, ri):
    return (f"M{f(cx-ro)} {f(cy)}a{f(ro)} {f(ro)} 0 1 0 {f(2*ro)} 0a{f(ro)} {f(ro)} 0 1 0 {f(-2*ro)} 0Z"
            f"M{f(cx-ri)} {f(cy)}a{f(ri)} {f(ri)} 0 1 1 {f(2*ri)} 0a{f(ri)} {f(ri)} 0 1 1 {f(-2*ri)} 0Z")


def rect(x, y, w, h):
    return f"M{f(x)} {f(y)}h{f(w)}v{f(h)}h{f(-w)}Z"


def arch(cx, a0, a1):
    """Band of the arch from angle a0 to a1 (degrees, SVG orientation), radial ends."""
    ro, ri = ARCH, ARCH - W
    p = lambda r, a: (cx + r * math.cos(math.radians(a)), MID + r * math.sin(math.radians(a)))
    (ox0, oy0), (ox1, oy1) = p(ro, a0), p(ro, a1)
    (ix1, iy1), (ix0, iy0) = p(ri, a1), p(ri, a0)
    large = 1 if (a1 - a0) > 180 else 0
    return (f"M{f(ox0)} {f(oy0)}A{f(ro)} {f(ro)} 0 {large} 1 {f(ox1)} {f(oy1)}"
            f"L{f(ix1)} {f(iy1)}A{f(ri)} {f(ri)} 0 {large} 0 {f(ix0)} {f(iy0)}Z")


def logotype(track=0.0):
    """track widens each of the four letter gaps, in units, for small sizes."""
    parts = []
    # o
    x = 0.0
    parts.append(ring(x + BOWL, MID, BOWL, BOWL - W))
    # r: stem and a shoulder that stops just past the top of the arch
    x += 2 * BOWL + 7 + track
    parts.append(rect(x, XH, W, BASE - XH))
    parts.append(arch(x + ARCH, 180, 292))
    # d: bowl and full-height stem flush with the bowl's outer edge
    x += ARCH + ARCH * math.cos(math.radians(292)) + 4.5 + track
    cx = x + BOWL
    parts.append(ring(cx, MID, BOWL, BOWL - W))
    parts.append(rect(cx + BOWL - W, 0, W, BASE))
    # i: stem and a round dot, like the crew in the mark
    x = cx + BOWL + 9 + track
    parts.append(rect(x, XH, W, BASE - XH))
    parts.append(f'M{f(x + W/2 - 4.6)} 6a4.6 4.6 0 1 0 9.2 0a4.6 4.6 0 1 0 -9.2 0Z')
    # n: stem, arch, right leg
    x += W + 9 + track
    parts.append(rect(x, XH, W, BASE - XH))
    parts.append(arch(x + ARCH, 180, 360))
    parts.append(rect(x + 2 * ARCH - W, MID, W, BASE - MID))
    width = x + 2 * ARCH
    return "".join(f'<path fill-rule="evenodd" d="{d}"/>' for d in parts), width


# Below 32px tall the letters crowd, so a small-size logotype opens each gap.
TRACK_SMALL = 4.0
WORD, WORD_W = logotype()
WORD_SMALL, WORD_SMALL_W = logotype(TRACK_SMALL)
word_box = lambda w: f"0 -1 {f(w + 1)} 58.6"
WORD_BOX, WORD_SMALL_BOX = word_box(WORD_W), word_box(WORD_SMALL_W)
MARK_SIZE, MARK_GAP = 80.0, 18.0


def lockup(form, word):
    return (f'<g transform="translate(0 {f(MID - MARK_SIZE/2)}) scale({f(MARK_SIZE/64)})">{FORMS[form]}</g>'
            f'<g transform="translate({f(MARK_SIZE + MARK_GAP)} 0)">{word}</g>')


lock_box = lambda w: f"0 {f(MID - MARK_SIZE/2)} {f(MARK_SIZE + MARK_GAP + w + 1)} {f(MARK_SIZE)}"
# As with the mark alone, a lockup shown under 48px tall uses the compact mark,
# and its word is then under 32px tall, so it takes the small-size logotype.
LOCK, LOCK_BOX = lockup("full", WORD), lock_box(WORD_W)
LOCK_COMPACT, LOCK_COMPACT_BOX = lockup("compact", WORD_SMALL), lock_box(WORD_SMALL_W)


def wide_svg(inner, box, height=None, cls=""):
    w = float(box.split()[2]); h = float(box.split()[3])
    sz = f' height="{height}" width="{f(height * w / h)}"' if height else ""
    c = f' class="{cls}"' if cls else ""
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{box}"{sz}{c} '
            f'fill="currentColor" role="img" aria-label="ordin">{inner}</svg>')


for name, inner, box in (("logotype", WORD, WORD_BOX), ("logotype-small", WORD_SMALL, WORD_SMALL_BOX),
                         ("lockup", LOCK, LOCK_BOX), ("lockup-compact", LOCK_COMPACT, LOCK_COMPACT_BOX)):
    with open(os.path.join(OUT, f"ordin-{name}.svg"), "w") as fh:
        fh.write(wide_svg(inner, box) + "\n")


def tile(inner, px, bg, fg, scale=0.66):
    # macOS-style rounded square, ~22.4% corner radius
    return (f'<span class="tile" style="width:{px}px;height:{px}px;border-radius:{px*0.224:.1f}px;'
            f'background:{bg};color:{fg}">{svg(inner, round(px*scale))}</span>')


def fig(body, cap):
    return f"<figure>{body}<figcaption>{cap}</figcaption></figure>"


def section():
    t = lambda form, px, s=0.66: tile(FORMS[form], px, "var(--icon-bg)", "var(--icon-fg)", s)
    hero = "".join(fig(svg(FORMS[k], 176, "accent"), c) for k, c in (
        ("full", "full · 48px and up"), ("compact", "compact · 24 to 47px"), ("small", "small · under 24px")))
    sizes = "".join([
        fig(t("full", 256), "256 · full"),
        fig(t("full", 128), "128 · full"),
        fig(t("full", 64), "64 · full"),
        fig(t("compact", 32), "32 · compact"),
        fig(t("full", 32), "32 · full, for comparison"),
        fig(t("small", 16, 0.8), "16 · small"),
    ])
    ui = "".join([
        fig(f'<span class="rail"><span class="badge">{svg(FORMS["compact"], 20)}</span>'
            f'<span class="badge">{svg(FORMS["small"], 18)}</span></span>', "28 side-rail badge · compact / small"),
        fig(f'<span class="menubar">{svg(FORMS["small"], 16)}<span>10:42</span></span>', "16 menu bar · one colour"),
    ])
    lock = (
        '<div class="lockup"><h3>Logotype and lockup</h3>'
        f'<div class="row">{wide_svg(LOCK, LOCK_BOX, 72)}</div>'
        f'<div class="row">{wide_svg(LOCK, LOCK_BOX, 72, "accent")}</div>'
        '<div class="row sizes">'
        + "".join(fig(wide_svg(WORD, WORD_BOX, h), f"{h}px tall") for h in (64, 32))
        + "".join(fig(wide_svg(WORD_SMALL, WORD_SMALL_BOX, h), f"{h}px tall · small spacing") for h in (20, 14))
        + fig(wide_svg(LOCK_COMPACT, LOCK_COMPACT_BOX, 28), "compact lockup · 28px tall")
        + '</div><div class="row spacing">'
        + "".join(fig(f'{wide_svg(WORD, WORD_BOX, h)}{wide_svg(WORD_SMALL, WORD_SMALL_BOX, h)}',
                      f"{h}px tall · regular (top) and small spacing (bottom)") for h in (14, 18, 24))
        + '</div><div class="row specimen">'
        f'<span class="sys">ordin</span>{wide_svg(WORD, WORD_BOX, 40)}'
        '<span class="cap">system font (left) and the drawn logotype (right), for comparison</span></div></div>')
    return f"""
<section class="variant">
  <header><h2>A · Open orbit, with text lines</h2>
  <p>The crew orbits a document. The two text lines make the page read as a piece of work, not a file, and drop out below 48px, where they would blur.</p></header>
  <div class="hero">{hero}</div>
  <div class="sizes">{sizes}</div>
  <div class="sizes ui">{ui}</div>
  {lock}
</section>"""


# Clear space: X is half the height of the logotype's o, which is also a quarter
# of the mark's height in the lockup. The mark alone keeps a quarter of its own
# height. Clear space is measured from the edge of each file's artwork box.
X_WORD = (BASE - XH) / 2       # 20 units: half of the o
X_MARK = 64 / 4                # 16 units on the mark's grid


def clear_space_svg(inner, box, x, height, dim=None):
    bx, by, bw, bh = (float(v) for v in box.split())
    ox, oy, ow, oh = bx - x, by - x, bw + 2 * x, bh + 2 * x
    sq = lambda px, py: (f'<rect x="{f(px)}" y="{f(py)}" width="{f(x)}" height="{f(x)}" class="xsq"/>'
                         f'<text x="{f(px + x/2)}" y="{f(py + x/2)}" class="xlab" font-size="{f(x*0.55)}">X</text>')
    marks = (sq(ox, by + bh/2 - x/2) + sq(bx + bw, by + bh/2 - x/2)
             + sq(bx + bw/2 - x/2, oy) + sq(bx + bw/2 - x/2, by + bh))
    measure = ""
    if dim:  # a bracket showing what X is measured from
        mx, y0, y1 = dim
        measure = (f'<path class="dim" d="M{f(mx-3)} {f(y0)}H{f(mx+3)}M{f(mx)} {f(y0)}V{f(y1)}M{f(mx-3)} {f(y1)}H{f(mx+3)}"/>')
    width = height * ow / oh
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{f(ox)} {f(oy)} {f(ow)} {f(oh)}" '
            f'width="{f(width)}" height="{height}" class="clear" role="img" aria-label="Clear space around the artwork">'
            f'<rect x="{f(ox)}" y="{f(oy)}" width="{f(ow)}" height="{f(oh)}" class="zone"/>'
            f'<rect x="{f(bx)}" y="{f(by)}" width="{f(bw)}" height="{f(bh)}" class="box"/>'
            f'{marks}<g class="art" fill="currentColor">{inner}</g>{measure}</svg>')


def usage():
    o_left = MARK_SIZE + MARK_GAP
    gap_in = WORD_W - 2 * ARCH - 4.5   # between the i and the n: the bracket shows X
    lock_dim = (o_left + gap_in, MID, BASE)
    word_dim = (gap_in, MID, BASE)
    diagrams = "".join([
        fig(clear_space_svg(LOCK, LOCK_BOX, X_WORD, 150, lock_dim), "lockup · X = half the height of the o"),
        fig(clear_space_svg(FORMS["full"], "0 0 64 64", X_MARK, 150), "logo · X = a quarter of its height"),
        fig(clear_space_svg(WORD, WORD_BOX, X_WORD, 110, word_dim), "logotype · X = half the height of the o"),
    ])
    mm = lambda px: f"{px * 25.4 / 96:.1f} mm"
    rows = [
        ("Logo", "16px", mm(16), svg(FORMS["small"], 16, "ink"),
         "Not shown smaller. Use the small form from 16 to 23px and the compact form from 24 to 47px."),
        ("Logotype", "14px tall", mm(14), wide_svg(WORD_SMALL, WORD_SMALL_BOX, 14, "ink"),
         "Not shown smaller; use the product name as ordinary text instead. Use the small-spacing logotype from 14 to 31px."),
        ("Lockup", "24px tall", mm(24), wide_svg(LOCK_COMPACT, LOCK_COMPACT_BOX, 24, "ink"),
         "Use the compact lockup from 24 to 47px. Below 24px, use the logo alone."),
    ]
    table = "".join(f"<tr><th scope=\"row\">{a}</th><td>{b}</td><td>{c}</td><td class=\"sample\">{d}</td><td>{e}</td></tr>"
                    for a, b, c, d, e in rows)
    return f"""
<section class="variant" id="usage">
  <header><h2>Clear space and minimum size</h2>
  <p>Keep the shaded area free of text, other logos and the edge of the window or page. Clear space is measured from the edge of the artwork box in each SVG file, so it can be applied without measuring the drawing.</p></header>
  <div class="hero clearing">{diagrams}</div>
  <ul class="rules">
    <li><strong>Lockup and logotype:</strong> X is half the height of the letter o. In the lockup this equals a quarter of the logo's height, so one measure covers both.</li>
    <li><strong>Logo alone:</strong> X is a quarter of the logo's height: 4px around a 16px logo, 16px around a 64px logo.</li>
    <li><strong>App icons</strong> are exempt. Their tile already carries the platform's own margin.</li>
    <li>Clear space is a minimum. More is always allowed.</li>
  </ul>
  <h3>Minimum sizes</h3>
  <table>
    <thead><tr><th scope="col">Artwork</th><th scope="col">Screen</th><th scope="col">Print, derived</th><th scope="col">At minimum</th><th scope="col">Smaller than that</th></tr></thead>
    <tbody>{table}</tbody>
  </table>
  <p class="cap">Screen sizes are in CSS pixels, so they hold on high-resolution displays. Print sizes are converted at 96 pixels to the inch and have not been tested on paper.</p>
</section>"""


HTML = f"""<!doctype html>
<html lang="en-GB">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>ordin logo</title>
<style>
:root{{
  --ground:#F2F4F7;--surface:#FFFFFF;--line:#DDE2E8;--ink:#17212B;--muted:#52606D;
  --accent:#1F4FB8;--zone:#E3EAF7;--icon-bg:#1F4FB8;--icon-fg:#FFFFFF;--primary-bg:#17212B;--primary-fg:#FFFFFF;
  --sans:-apple-system,BlinkMacSystemFont,"Segoe UI",system-ui,sans-serif;
}}
@media (prefers-color-scheme: dark){{:root:not([data-theme="light"]){{
  --ground:#11161C;--surface:#1A2129;--line:#2C3642;--ink:#E8EDF2;--muted:#9AA6B3;
  --accent:#7FA6F5;--zone:#22314A;--icon-bg:#22314A;--icon-fg:#A9C3FA;--primary-bg:#E8EDF2;--primary-fg:#11161C;
}}}}
:root[data-theme="dark"]{{
  --ground:#11161C;--surface:#1A2129;--line:#2C3642;--ink:#E8EDF2;--muted:#9AA6B3;
  --accent:#7FA6F5;--zone:#22314A;--icon-bg:#22314A;--icon-fg:#A9C3FA;--primary-bg:#E8EDF2;--primary-fg:#11161C;
}}
*{{box-sizing:border-box}}
body{{margin:0;background:var(--ground);color:var(--ink);font:15px/1.5 var(--sans)}}
main{{max-width:1080px;margin:0 auto;padding:24px 16px 64px}}
.top{{display:flex;justify-content:space-between;align-items:flex-start;gap:16px;flex-wrap:wrap}}
h1{{font-size:22px;margin:0 0 4px}} .top p{{margin:0;color:var(--muted);max-width:62ch}}
button{{font:inherit;border:1px solid var(--line);background:var(--surface);color:var(--ink);border-radius:8px;padding:6px 12px;cursor:pointer}}
.variant{{background:var(--surface);border:1px solid var(--line);border-radius:14px;padding:20px;margin-top:20px}}
.variant h2{{font-size:17px;margin:0}} .variant header p{{margin:2px 0 0;color:var(--muted)}}
.hero{{display:flex;gap:24px;flex-wrap:wrap;margin:16px 0;padding:16px;border-radius:10px;background:var(--ground)}}
.hero svg{{max-width:26vw;height:auto}} .ui{{margin-top:18px}}
.accent{{color:var(--accent)}} .faint{{opacity:.55}}
.sizes{{display:flex;gap:22px;align-items:flex-end;flex-wrap:wrap}}
figure{{margin:0;display:flex;flex-direction:column;align-items:center;gap:6px}}
figcaption{{font-size:12px;color:var(--muted)}}
.tile{{display:flex;align-items:center;justify-content:center}}
.rail{{display:flex;gap:8px;padding:8px;border-radius:10px;background:var(--ground)}}
.badge{{width:28px;height:28px;border-radius:8px;background:var(--primary-bg);color:var(--primary-fg);display:flex;align-items:center;justify-content:center}}
.menubar{{display:flex;align-items:center;gap:8px;height:24px;padding:0 10px;border-radius:6px;background:#E6E6E6;color:#000;font-size:12px}}
@media (prefers-color-scheme: dark){{:root:not([data-theme="light"]) .menubar{{background:#2B2B2B;color:#fff}}}}
:root[data-theme="dark"] .menubar{{background:#2B2B2B;color:#fff}}
.lockup{{margin-top:20px;padding-top:16px;border-top:1px solid var(--line)}}
.lockup h3{{font-size:15px;margin:0 0 12px}} .lockup .row{{margin:14px 0}} .lockup svg{{max-width:100%;height:auto}}
.spacing{{display:flex;gap:28px;flex-wrap:wrap;align-items:flex-end}} .spacing figure svg{{display:block;margin:3px 0}}
.specimen{{display:flex;align-items:center;gap:28px;flex-wrap:wrap}} .sys{{font-size:46px;font-weight:600;letter-spacing:-.02em;line-height:1}}
.cap{{font-size:12px;color:var(--muted);flex-basis:100%}}
.clearing{{align-items:flex-end}} .clearing svg{{max-width:100%;height:auto}}
.clear .zone{{fill:var(--zone)}} .clear .box{{fill:var(--surface);stroke:var(--accent);stroke-width:.4;stroke-dasharray:1.5 1.5}}
.clear .xsq{{fill:none;stroke:var(--accent);stroke-width:.5}} .clear .xlab{{fill:var(--accent);text-anchor:middle;dominant-baseline:central;font-family:var(--sans)}}
.clear .art{{color:var(--ink)}} .clear .dim{{fill:none;stroke:var(--accent);stroke-width:.8}} .ink{{color:var(--ink)}}
.rules{{margin:8px 0 16px;padding-left:20px}} .rules li{{margin:4px 0}} #usage h3{{font-size:15px;margin:16px 0 8px}}
table{{border-collapse:collapse;width:100%;font-size:14px}} th,td{{text-align:start;padding:8px 10px;border-bottom:1px solid var(--line);vertical-align:middle}}
thead th{{color:var(--muted);font-weight:600}} .sample svg{{display:block}}
@media (max-width:640px){{table{{display:block;overflow-x:auto}}}}
.notes{{margin-top:24px;color:var(--muted);font-size:14px}} .notes li{{margin:4px 0}}
</style>
</head>
<body>
<main>
<div class="top">
  <div><h1>ordin logo · variant A and logotype</h1>
  <p>One colour (currentColor) on a 64-unit grid, in three forms chosen by the size it is shown at.</p></div>
  <button id="theme" type="button">Switch light / dark</button>
</div>
{section()}
{usage()}
<ul class="notes">
  <li>The colours are the interface concept's tokens: accent #1F4FB8 in light, #7FA6F5 in dark. No amber or red, which mean "waiting" and "danger".</li>
  <li>The logotype is drawn as outlines, so it looks the same on every computer and needs no font file. It shares the mark\'s round geometry: the bowls of o and d, the arches of r and n, and a round dot on the i like the crew.</li>
  <li>The logo is not mirrored for right-to-left layouts. The shorter second line suggests left-to-right text, which is the usual convention for a document icon.</li>
</ul>
</main>
<script>
document.getElementById('theme').addEventListener('click',function(){{
  var r=document.documentElement,cur=r.getAttribute('data-theme');
  var dark=cur?cur==='dark':matchMedia('(prefers-color-scheme: dark)').matches;
  r.setAttribute('data-theme',dark?'light':'dark');
}});
</script>
</body>
</html>
"""

with open(os.path.join(OUT, "index.html"), "w") as fh:
    fh.write(HTML)
print("written", sorted(os.listdir(OUT)))
