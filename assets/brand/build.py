"""Generate hidforge brand SVGs (mouse + keyhole mark, wordmark, lockups)."""
import math, sys
from pathlib import Path
from fontTools.ttLib import TTFont
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen

OUT = Path(sys.argv[1])
OUT.mkdir(parents=True, exist_ok=True)

FOREST = "#182825"   # RAIL
GREEN = "#205b4e"    # ACTION
PAPER = "#f5f7ef"    # RAIL_INK
AMBER = "#ffc48e"    # RAIL_WARN

f = lambda v: f"{v:.2f}".rstrip("0").rstrip(".")


def mark(split_half, key_r, key_cy, stem_top_half, stem_bot_half, stem_bot_y):
    """Mouse body (pill 136x200 at 60,28) with button split flowing into a keyhole."""
    cx, R, top_cy, bot_cy = 128, 68, 96, 160
    xl, xr = cx - split_half, cx + split_half
    body_y = top_cy - math.sqrt(R * R - split_half ** 2)
    key_y = key_cy - math.sqrt(key_r ** 2 - split_half ** 2)
    stem_y = key_cy + math.sqrt(key_r ** 2 - stem_top_half ** 2)
    outer = (
        f"M{f(xl)} {f(body_y)}A{R} {R} 0 0 0 60 {top_cy}L60 {bot_cy}"
        f"A{R} {R} 0 0 0 128 228A{R} {R} 0 0 0 196 {bot_cy}L196 {top_cy}"
        f"A{R} {R} 0 0 0 {f(xr)} {f(body_y)}"
        f"L{f(xr)} {f(key_y)}A{key_r} {key_r} 0 0 1 {f(cx + stem_top_half)} {f(stem_y)}"
        f"L{f(cx + stem_bot_half)} {stem_bot_y}L{f(cx - stem_bot_half)} {stem_bot_y}"
        f"L{f(cx - stem_top_half)} {f(stem_y)}A{key_r} {key_r} 0 0 1 {f(xl)} {f(key_y)}Z"
    )
    # keyhole only (for the amber fill in the app icon)
    hole = (
        f"M{f(cx - stem_top_half)} {f(stem_y)}A{key_r} {key_r} 0 1 1 {f(cx + stem_top_half)} {f(stem_y)}"
        f"L{f(cx + stem_bot_half)} {stem_bot_y}L{f(cx - stem_bot_half)} {stem_bot_y}Z"
    )
    return outer, hole


MAIN = mark(5, 22, 104, 9, 15, 164)
SMALL = mark(9, 28, 108, 12, 20, 172)


def svg(view, body, title="hidforge"):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{view}">'
            f"<title>{title}</title>{body}</svg>\n")


def write(name, text):
    (OUT / name).write_text(text)


# --- wordmark --------------------------------------------------------------
font = TTFont("/usr/share/fonts/TTF/FiraSans-SemiBold.ttf")
gs, cmap = font.getGlyphSet(), font.getBestCmap()
upm = font["head"].unitsPerEm
hmtx = font["hmtx"]
kern = {}
TEXT = "hidforge"


def wordmark_path(x0, baseline, size, tracking=-0.01):
    s = size / upm
    pen = SVGPathPen(gs, ntos=f)
    x = 0.0
    for ch in TEXT:
        g = cmap[ord(ch)]
        tp = TransformPen(pen, (s, 0, 0, -s, x0 + x * s, baseline))
        gs[g].draw(tp)
        x += hmtx[g][0] + tracking * upm
    return pen.getCommands(), (x - tracking * upm) * s


ASC = 0.69 * upm  # approx ascender height of 'h' in Fira Sans


def ascender_units():
    from fontTools.pens.boundsPen import BoundsPen
    bp = BoundsPen(gs)
    gs[cmap[ord("h")]].draw(bp)
    return bp.bounds[3]


H = ascender_units()

# horizontal lockup: 'h' ascender spans 62..196 (134 tall), centered on mark
size_h = 134 / H * upm
base_h = 196
wm_h, w_h = wordmark_path(244, base_h, size_h)
right = 244 + w_h
desc = base_h + 0.24 * size_h

# stacked lockup: mark on top, wordmark centered below
size_s = 72 / H * upm
_, w_s = wordmark_path(0, 0, size_s)
x_s = 128 - w_s / 2
wm_s, _ = wordmark_path(x_s, 316, size_s)

# standalone wordmark
wm_only, w_o = wordmark_path(0, 134, size_h)


def variants(prefix, outer, hole):
    write(f"{prefix}.svg", svg("0 0 256 256", f'<path fill="{GREEN}" d="{outer}"/>'))
    write(f"{prefix}-black.svg", svg("0 0 256 256", f'<path fill="#000" d="{outer}"/>'))
    write(f"{prefix}-white.svg", svg("0 0 256 256", f'<path fill="#fff" d="{outer}"/>'))
    write(f"{prefix}-tinted.svg", svg("0 0 256 256", f'<path fill="currentColor" d="{outer}"/>'))
    # app icon: forest tile, paper mouse, amber light through the keyhole
    write(f"{prefix}-app-icon.svg", svg("0 0 256 256",
        f'<rect width="256" height="256" rx="56" fill="{FOREST}"/>'
        f'<g transform="translate(128 128) scale(.8) translate(-128 -128)">'
        f'<path fill="{AMBER}" d="{hole}"/><path fill="{PAPER}" d="{outer}"/></g>'))


variants("hidforge-mark", *MAIN)
variants("hidforge-mark-small", *SMALL)

outer = MAIN[0]
vb_h = f"40 0 {f(right + 20 - 40)} 256"
write("hidforge-horizontal.svg", svg(vb_h,
    f'<path fill="{GREEN}" d="{outer}"/><path fill="{FOREST}" d="{wm_h}"/>'))
write("hidforge-horizontal-black.svg", svg(vb_h, f'<path fill="#000" d="{outer} {wm_h}"/>'))
write("hidforge-horizontal-white.svg", svg(vb_h, f'<path fill="#fff" d="{outer} {wm_h}"/>'))
write("hidforge-horizontal-reversed.svg", svg(vb_h,
    f'<path fill="{AMBER}" d="{MAIN[1]}"/><path fill="{PAPER}" d="{outer}"/><path fill="{PAPER}" d="{wm_h}"/>'))

left_s = min(40, x_s - 28)
vb_s = f"{f(left_s)} 12 {f(256 - 2 * left_s)} 336"
write("hidforge-stacked.svg", svg(vb_s,
    f'<path fill="{GREEN}" d="{outer}"/><path fill="{FOREST}" d="{wm_s}"/>'))
write("hidforge-stacked-black.svg", svg(vb_s, f'<path fill="#000" d="{outer} {wm_s}"/>'))
write("hidforge-stacked-white.svg", svg(vb_s, f'<path fill="#fff" d="{outer} {wm_s}"/>'))

write("hidforge-wordmark.svg", svg(f"-8 0 {f(w_o + 16)} 190", f'<path fill="{FOREST}" d="{wm_only}"/>'))
print("ok", OUT)
