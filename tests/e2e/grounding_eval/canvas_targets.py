"""The grounding evaluation's canvas set (c17): 60 targets that have no element of their own, painted on a
<canvas> or inside an <img>, on six synthetic pages. The snapshot lists only actionable roles and headings, so a
canvas or picture can't be found by role and name. Each page instead has a heading pinned at the page's top-left
(absolute, fixed height) as the anchor, found like any other target, and the surface at a fixed offset from it; a
target's `box_in_anchor` is its box from the heading's top-left in CSS pixels (screen pixels at the Desktop's 100 %
zoom and device pixel ratio 1), and the capture adds the heading's screen position. The pages draw from the same
spec as the boxes, so the two can't drift.

Deterministic: the same seed gives the same pages, targets and order. `targets.py OUT --canvas` writes them after the
200 (`build()` gives the files by name, `.html` and `.svg`, and the targets).
"""

import html
import json
import random
import re

SEED = 20261021
PER_PAGE = 10
# The browser's view on the Desktop at 1280 x 800, less its own bars; every anchor must fit inside it, unscrolled.
VIEW = (1240, 640)
SURFACE_AT = (24, 40)  # where the canvas or picture sits, from the anchor heading's top-left


def spec_box(x, y, w, h):
    return [int(x), int(y), int(w), int(h)]


def toolbar(rng):
    """A drawing app's tool strip painted on a canvas: glyph-only tools, no labels."""
    tools = [("scissors", "✂"), ("pencil", "✎"), ("eraser", "⌫"), ("ink drop", "\U0001F4A7"),
             ("text tool", "T"), ("magnifier", "\U0001F50D"), ("hand", "✋"), ("undo arrow", "↶"),
             ("redo arrow", "↷"), ("trash can", "\U0001F5D1"), ("star", "★"), ("lock", "\U0001F512")]
    chosen = rng.sample(tools, PER_PAGE)
    size, gap = 36, rng.randint(6, 14)
    shapes, targets = [], []
    for index, (what, glyph) in enumerate(chosen):
        box = spec_box(12 + index * (size + gap), 12, size, size)
        shapes.append({"kind": "button", "box": box, "glyph": glyph})
        targets.append((f"Click the {what} tool in the painted toolbar.", what, box))
    canvas = (12 * 2 + PER_PAGE * (size + gap), 60)
    return "Painted toolbar", canvas, shapes, targets, False


def palette(rng):
    """Colour swatches and brush sizes, told apart only by colour or size."""
    swatches = [("red", "#d32f2f"), ("dark green", "#1b5e20"), ("light blue", "#81d4fa"), ("yellow", "#fdd835"),
                ("purple", "#7b1fa2"), ("orange", "#f57c00"), ("black", "#000000"), ("pink", "#f48fb1"),
                ("brown", "#6d4c41"), ("grey", "#9e9e9e")]
    chosen = rng.sample(swatches, 7)
    shapes, targets = [], []
    for index, (name, colour) in enumerate(chosen):
        box = spec_box(16 + (index % 4) * 56, 16 + (index // 4) * 56, 44, 44)
        shapes.append({"kind": "swatch", "box": box, "fill": colour})
        targets.append((f"Pick the {name} swatch in the colour palette.", f"{name} swatch", box))
    for index, (name, radius) in enumerate([("smallest", 3), ("medium", 7), ("largest", 12)]):
        box = spec_box(260 + index * 44, 30, 32, 32)
        shapes.append({"kind": "brush", "box": box, "radius": radius})
        targets.append((f"Choose the {name} brush size.", f"{name} brush", box))
    return "Palette", (420, 140), shapes, targets, False


def chart(rng):
    """A bar chart with a painted legend: months' bars and the legend's entries."""
    months = ["January", "February", "March", "April", "May", "June", "July"]
    shapes, targets = [], []
    width, height, base = 640, 360, 320
    for index, month in enumerate(months):
        value = rng.randint(60, 260)
        box = spec_box(60 + index * 58, base - value, 44, value)
        shapes.append({"kind": "bar", "box": box, "fill": "#3f51b5", "label": month[:3],
                       "label_at": [60 + index * 58 + 22, base + 22]})
        targets.append((f"Click the bar for {month} in the chart.", f"{month} bar", box))
    for index, (series, colour) in enumerate([("Revenue", "#3f51b5"), ("Costs", "#e53935"), ("Forecast", "#43a047")]):
        box = spec_box(width - 150, 20 + index * 34, 130, 26)
        shapes.append({"kind": "legend", "box": box, "fill": colour, "text": series})
        targets.append((f"Click “{series}” in the chart's legend.", f"{series} legend entry", box))
    shapes.insert(0, {"kind": "axes", "base": base, "left": 40, "right": width - 170})
    return "Chart", (width, height), shapes, targets[:PER_PAGE], False


def board(rng):
    """A game board on a canvas: squares named by their file and rank, painted labels on the edges."""
    size, cell, left, top = 8, 44, 30, 10
    shapes = [{"kind": "board", "left": left, "top": top, "cell": cell, "size": size}]
    squares = rng.sample([(f, r) for f in range(size) for r in range(size)], PER_PAGE)
    targets = []
    for file_, rank in squares:
        name = f"{'abcdefgh'[file_]}{rank + 1}"
        box = spec_box(left + file_ * cell, top + (size - 1 - rank) * cell, cell, cell)
        targets.append((f"Click square {name} on the game board.", f"square {name}", box))
    return "Game board", (left + size * cell + 10, top + size * cell + 30), shapes, targets, False


def image_buttons(rng):
    """Buttons drawn inside an SVG picture shown with <img>: the picture is one element, its buttons none."""
    labels = ["Play", "Sign in", "Next", "Back", "Skip intro", "Settings", "Buy now", "Share", "Close", "Help"]
    rng.shuffle(labels)
    shapes, targets = [], []
    for index, label in enumerate(labels):
        box = spec_box(20 + (index % 5) * 150, 40 + (index // 5) * 120, 120, 44)
        shapes.append({"kind": "svgbutton", "box": box, "text": label})
        targets.append((f"Press the “{label}” button in the picture.", f"{label} picture button", box))
    return "Picture buttons", (780, 240), shapes, targets, False


def dark_dialog(rng):
    """A dark, painted dialog with English and Chinese buttons and a close cross."""
    buttons = [("OK", "OK"), ("Cancel", "Cancel"), ("Apply", "Apply"), ("保存", "保存 (Save)"),
               ("取消", "取消 (Cancel)"), ("下一步", "下一步 (Next)"), ("删除", "删除 (Delete)"), ("Retry", "Retry"),
               ("Details", "Details")]
    chosen = rng.sample(buttons, PER_PAGE - 1)
    shapes = [{"kind": "dialog", "box": spec_box(0, 0, 560, 300), "title": "Unsaved changes"}]
    targets = []
    close = spec_box(560 - 36, 8, 28, 28)
    shapes.append({"kind": "close", "box": close})
    targets.append(("Close the painted dialog with its cross.", "dialog close cross", close))
    for index, (text, said) in enumerate(chosen):
        box = spec_box(24 + (index % 3) * 176, 120 + (index // 3) * 56, 156, 40)
        shapes.append({"kind": "button", "box": box, "text": text})
        targets.append((f"Press the {said} button in the dialog.", f"{said} dialog button", box))
    return "Painted dialog", (560, 300), shapes, targets, True


PAGES = [toolbar, palette, chart, board, image_buttons, dark_dialog]

DRAW = r"""
const spec = JSON.parse(document.getElementById('spec').textContent);
const c = document.getElementById('surface'); const g = c.getContext('2d');
const dark = spec.dark, ink = dark ? '#e6e6e6' : '#1a1a1a', face = dark ? '#2a2f36' : '#f0f0f0';
g.fillStyle = dark ? '#111418' : '#ffffff'; g.fillRect(0, 0, c.width, c.height);
g.textAlign = 'center'; g.textBaseline = 'middle';
function rect([x, y, w, h], fill, stroke) { g.fillStyle = fill; g.fillRect(x, y, w, h);
  if (stroke) { g.strokeStyle = stroke; g.strokeRect(x + .5, y + .5, w - 1, h - 1); } }
function label([x, y, w, h], text, size) { g.fillStyle = ink; g.font = size + 'px sans-serif';
  g.fillText(text, x + w / 2, y + h / 2); }
for (const s of spec.shapes) {
  if (s.kind === 'button') { rect(s.box, face, '#888'); label(s.box, s.glyph || s.text, s.glyph ? 20 : 15); }
  else if (s.kind === 'swatch') rect(s.box, s.fill, '#444');
  else if (s.kind === 'brush') { rect(s.box, face, '#888'); g.beginPath(); g.fillStyle = ink;
    g.arc(s.box[0] + s.box[2] / 2, s.box[1] + s.box[3] / 2, s.radius, 0, 7); g.fill(); }
  else if (s.kind === 'axes') { g.strokeStyle = ink; g.beginPath(); g.moveTo(s.left, 10); g.lineTo(s.left, s.base);
    g.lineTo(s.right, s.base); g.stroke(); }
  else if (s.kind === 'bar') { rect(s.box, s.fill); g.fillStyle = ink; g.font = '13px sans-serif';
    g.fillText(s.label, s.label_at[0], s.label_at[1]); }
  else if (s.kind === 'legend') { rect(s.box, face, '#888'); rect([s.box[0] + 6, s.box[1] + 7, 12, 12], s.fill);
    g.fillStyle = ink; g.font = '13px sans-serif'; g.textAlign = 'left';
    g.fillText(s.text, s.box[0] + 26, s.box[1] + s.box[3] / 2); g.textAlign = 'center'; }
  else if (s.kind === 'board') { for (let f = 0; f < s.size; f++) for (let r = 0; r < s.size; r++)
      rect([s.left + f * s.cell, s.top + r * s.cell, s.cell, s.cell], (f + r) % 2 ? '#b58863' : '#f0d9b5');
    g.fillStyle = ink; g.font = '12px sans-serif';
    for (let f = 0; f < s.size; f++) g.fillText('abcdefgh'[f], s.left + f * s.cell + s.cell / 2,
      s.top + s.size * s.cell + 14);
    for (let r = 0; r < s.size; r++) g.fillText(String(s.size - r), s.left - 14, s.top + r * s.cell + s.cell / 2); }
  else if (s.kind === 'dialog') { rect(s.box, face, '#666'); g.fillStyle = ink; g.font = 'bold 16px sans-serif';
    g.textAlign = 'left'; g.fillText(spec.dialog_title, 24, 24); g.font = '14px sans-serif';
    g.fillText('Save your changes before closing?', 24, 70); g.textAlign = 'center'; }
  else if (s.kind === 'close') { rect(s.box, face); label(s.box, '✕', 16); }
}
"""


def page_html(title, anchor_name, size, shapes, dark, svg_file=None):
    colours = ("#111418", "#e6e6e6") if dark else ("#ffffff", "#1a1a1a")
    left, top = SURFACE_AT
    head = (f"<!doctype html><html lang=en><meta charset=utf-8><title>{html.escape(title)}</title>"
            f"<style>body{{margin:0;font:14px sans-serif;background:{colours[0]};color:{colours[1]}}}"
            "h1{position:absolute;left:0;top:0;margin:0;padding:0 0 0 24px;width:600px;height:24px;"
            "line-height:24px;font-size:14px;overflow:hidden;white-space:nowrap}"
            f"canvas,img{{display:block;position:absolute;left:{left}px;top:{top}px}}</style><body>"
            f"<h1>{html.escape(anchor_name)}</h1>")
    if svg_file:
        # The picture is one element, named by its alt text; nothing inside it is in the DOM or the tree.
        return head + (f"<img src='{svg_file}' alt='' width={size[0]} height={size[1]}>"
                       "</body></html>")
    spec = {"shapes": shapes, "dark": dark,
            "dialog_title": next((s["title"] for s in shapes if s["kind"] == "dialog"), "")}
    # "</" can't close the script early.
    text = json.dumps(spec).replace("</", "<\\/")
    return head + (f"<canvas id=surface width={size[0]} height={size[1]}></canvas>"
                   f"<script type=application/json id=spec>{text}</script>"
                   f"<script>{DRAW}</script></body></html>")


def svg(size, shapes):
    parts = [f"<svg xmlns='http://www.w3.org/2000/svg' width='{size[0]}' height='{size[1]}'>",
             f"<rect width='{size[0]}' height='{size[1]}' fill='#eef3f8'/>",
             "<text x='20' y='24' font-family='sans-serif' font-size='15' fill='#1a1a1a'>Welcome back</text>"]
    for s in shapes:
        x, y, w, h = s["box"]
        parts.append(f"<rect x='{x}' y='{y}' width='{w}' height='{h}' rx='8' fill='#1565c0'/>")
        parts.append(f"<text x='{x + w / 2}' y='{y + h / 2 + 5}' text-anchor='middle' font-family='sans-serif'"
                     f" font-size='15' fill='#ffffff'>{html.escape(s['text'])}</text>")
    parts.append("</svg>")
    return "".join(parts)


def overlaps(a, b):
    return a[0] < b[0] + b[2] and b[0] < a[0] + a[2] and a[1] < b[1] + b[3] and b[1] < a[1] + a[3]


def build(seed=SEED):
    """The pages' files (name to text: each page's .html, and the picture page's .svg) and the 60 targets, in order."""
    rng = random.Random(seed)
    files, targets, sizes = {}, [], {}
    for number, make in enumerate(PAGES, start=1):
        title, size, shapes, chosen, dark = make(rng)
        page_id = f"canvas-{number:02d}"
        anchor_role, anchor_name = "heading", f"{title} ({page_id})"
        svg_file = None
        if make is image_buttons:
            svg_file = f"{page_id}.svg"
            files[svg_file] = svg(size, shapes)
        files[f"{page_id}.html"] = page_html(title, anchor_name, size, shapes, dark, svg_file)
        sizes[page_id] = size
        for index, (instruction, name, box) in enumerate(chosen, start=1):
            targets.append({"id": f"{page_id}-{index:02d}", "kind": "canvas", "page": page_id,
                            "instruction": instruction, "name": name,
                            "anchor": {"role": anchor_role, "name": anchor_name},
                            "box_in_anchor": [SURFACE_AT[0] + box[0], SURFACE_AT[1] + box[1], box[2], box[3]],
                            "dark": dark})
    check(files, targets, sizes)
    return files, targets


def check(files, targets, sizes=None):
    """The set's rules: ten targets on each of six pages, unique ids, every page's file there, every surface in the
    browser's view, and every box at least 12 px, inside its surface and apart from its page's others."""
    pages = sorted({item["page"] for item in targets})
    assert len(targets) == PER_PAGE * len(PAGES) and len(pages) == len(PAGES), (len(targets), pages)
    assert len({item["id"] for item in targets}) == len(targets)
    left, top = SURFACE_AT
    for page in pages:
        assert f"{page}.html" in files, page
        size = (sizes or {}).get(page) or surface(files[f"{page}.html"])
        assert left + size[0] <= VIEW[0] and top + size[1] <= VIEW[1], (page, size)
        boxes = [item["box_in_anchor"] for item in targets if item["page"] == page]
        assert len(boxes) == PER_PAGE, (page, len(boxes))
        for index, box in enumerate(boxes):
            x, y, width, height = box
            assert width >= 12 and height >= 12, (page, box)
            assert left <= x and top <= y and x + width <= left + size[0] and y + height <= top + size[1], (page, box)
            for other in boxes[:index]:
                assert not overlaps(box, other), (page, box, other)


def surface(text):
    """A page's surface size, from its canvas or img tag."""
    match = re.search(r"<(?:canvas id=surface|img src='[^']*' alt='') width=(\d+) height=(\d+)", text)
    assert match, "no surface"
    return int(match[1]), int(match[2])
