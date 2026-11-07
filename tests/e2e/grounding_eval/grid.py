"""Grid-then-zoom for the grounding evaluation (c17, exploratory; the computer tool has no grid yet): the model never
writes a coordinate. It sees the screen divided into labelled cells, names the cell holding what it wants to see it
closer, divided again, and clicks a fine cell by its label; the fine cell's centre is the point. Both variants of the
c17 design note, with its review's correction:
- G12: 8 x 5 cells of 160 px, then the chosen cell plus 10 px each side (180 x 180) in 15 x 15 cells of 12 px;
- G20: 8 x 5 of 160 px, then the cell plus 20 px (200 x 200) in 10 x 10 of 20 px, then, for something smaller than a
  cell, that cell plus 10 px (40 x 40) in 4 x 4 of 10 px.
- Z1000 (zoom + 0-1000, exploratory): level 1 as both, then the chosen cell plus 20 px (200 x 200) with no grid, the
  model clicking x and y from 0 to 1000 across that image (the 0-1000 convention of Arm 1, inside one zoom).
A window is shifted inside the screen at its edges, never cut. Columns are letters without I or O, rows numbers. The
drawing works from the capture's unmarked screen (standard library only: see pngio.py): level 1 at the screen's size,
each zoom scaled up to 640 x 640, 1 px lines and a corner label in each cell.
"""

import copy
import math

import pngio

COLUMNS = "ABCDEFGHJKLMNPQRSTUVWXYZ"
TOP = 160
ZOOM_SIDE = 640
# Per variant, the levels after level 1: (fine cell side, margin round the chosen cell).
VARIANTS = {"g12": [(12, 10)], "g20": [(20, 20), (10, 10)]}
# Per crop variant, the margin round the chosen level-1 cell; the crop has no grid and is clicked from 0 to 1000.
CROPS = {"z1000": 20}
CROP_MAX = 1000
LINE = (255, 0, 255)
LABEL_FILLS = ((198, 40, 40), (21, 101, 192))
LABEL_INK = (255, 255, 255)
# A 3 x 5 font for the labels: the columns' letters and the digits.
FONT = {
    "A": ["010", "101", "111", "101", "101"], "B": ["110", "101", "110", "101", "110"],
    "C": ["011", "100", "100", "100", "011"], "D": ["110", "101", "101", "101", "110"],
    "E": ["111", "100", "110", "100", "111"], "F": ["111", "100", "110", "100", "100"],
    "G": ["011", "100", "101", "101", "011"], "H": ["101", "101", "111", "101", "101"],
    "J": ["001", "001", "001", "101", "010"], "K": ["101", "101", "110", "101", "101"],
    "L": ["100", "100", "100", "100", "111"], "M": ["101", "111", "111", "101", "101"],
    "N": ["101", "111", "111", "111", "101"], "P": ["110", "101", "110", "100", "100"],
    "Q": ["010", "101", "101", "110", "011"],
    "0": ["111", "101", "101", "101", "111"], "1": ["010", "110", "010", "010", "111"],
    "2": ["110", "001", "010", "100", "111"], "3": ["110", "001", "010", "001", "110"],
    "4": ["101", "101", "111", "001", "001"], "5": ["111", "100", "110", "001", "110"],
    "6": ["011", "100", "111", "101", "111"], "7": ["111", "001", "010", "010", "010"],
    "8": ["111", "101", "111", "101", "111"], "9": ["111", "101", "111", "001", "110"],
}


class View:
    """One grid image: its window on the screen (x, y, width, height), its cell side in screen pixels, its columns
    and rows, its level (1 the whole screen), and the cells picked to reach it."""

    def __init__(self, window, cell, level, path, screen):
        self.window, self.cell, self.level, self.path, self.screen = window, cell, level, path, screen
        self.cols, self.rows = math.ceil(window[2] / cell), math.ceil(window[3] / cell)


def level1(width, height):
    return View((0, 0, width, height), TOP, 1, [], (width, height))


def label(col, row):
    return f"{COLUMNS[col]}{row + 1}"


def labels(view):
    return [label(col, row) for row in range(view.rows) for col in range(view.cols)]


def parse(view, text):
    """(col, row) of a label in this view, or None."""
    text = str(text or "").strip().upper()
    if len(text) < 2 or text[0] not in COLUMNS[:view.cols] or not text[1:].isdigit():
        return None
    row = int(text[1:]) - 1
    return (COLUMNS.index(text[0]), row) if 0 <= row < view.rows else None


def cell_rect(view, text):
    """A cell's rectangle on the screen, cut at the window's edge (a last row or column may be short)."""
    col, row = parse(view, text)
    x, y = view.window[0] + col * view.cell, view.window[1] + row * view.cell
    right, bottom = view.window[0] + view.window[2], view.window[1] + view.window[3]
    return x, y, min(view.cell, right - x), min(view.cell, bottom - y)


def centre(view, text):
    """The screen point a click on that cell lands on: the middle of a whole cell."""
    col, row = parse(view, text)
    return view.window[0] + col * view.cell + view.cell // 2, view.window[1] + row * view.cell + view.cell // 2


def around(view, text, margin):
    """The square window round a cell, `margin` wider on each side, shifted inside the screen."""
    col, row = parse(view, text)
    size = view.cell + 2 * margin
    x, y = view.window[0] + col * view.cell - margin, view.window[1] + row * view.cell - margin
    width, height = view.screen
    return min(max(x, 0), width - size), min(max(y, 0), height - size), size, size


def zoom(view, text, variant):
    """The view a grid call with this cell gives: the next level's window round it, or None past the last level."""
    if variant in CROPS:
        return crop(view, text, variant)
    levels = VARIANTS[variant]
    if view.level > len(levels) or parse(view, text) is None:
        return None
    side, margin = levels[view.level - 1]
    return View(around(view, text, margin), side, view.level + 1, view.path + [text.strip().upper()], view.screen)


def crop(view, text, variant):
    """A crop variant's zoom: from level 1 only, the cell plus its margin as one cell with no grid, or None."""
    if view.level != 1 or parse(view, text) is None:
        return None
    window = around(view, text, CROPS[variant])
    return View(window, window[2], 2, view.path + [text.strip().upper()], view.screen)


def crop_point(view, x, y):
    """The screen point of a click at (x, y) from 0 to 1000 across a crop, as the scorer maps Arm 1's 0-1000 (1000 is
    the far edge and counts as 999, then a pixel's centre on the screen); None for anything but whole numbers in
    range."""
    if not all(isinstance(v, int) and not isinstance(v, bool) and 0 <= v <= CROP_MAX for v in (x, y)):
        return None
    left, top, width, height = view.window
    x, y = min(x, CROP_MAX - 1), min(y, CROP_MAX - 1)
    return left + (2 * x + 1) * width // (2 * CROP_MAX), top + (2 * y + 1) * height // (2 * CROP_MAX)


def overlaps(window, box):
    x, y, w, h = window
    return box[0] < x + w and x < box[0] + box[2] and box[1] < y + h and y < box[1] + box[3]


def inside(point, box):
    return box[0] <= point[0] < box[0] + box[2] and box[1] <= point[1] < box[1] + box[3]


def reachable(box, variant, width, height, levels=None):
    """Whether a click reached from the level-1 cell holding the box's middle can land in it, at any level."""
    deepest = 1 + len(VARIANTS[variant]) if levels is None else levels
    top = level1(width, height)
    middle = (box[0] + box[2] / 2, box[1] + box[3] / 2)
    start = label(int(middle[0] // TOP), int(middle[1] // TOP))

    def search(view):
        if any(inside(centre(view, name), box) for name in labels(view)):
            return True
        if view.level >= deepest:
            return False
        return any(search(zoom(view, name, variant)) for name in labels(view)
                   if overlaps(zoom(view, name, variant).window, box))
    return inside(centre(top, start), box) or search(zoom(top, start, variant))


def render(screen, view, marks=True):
    """The grid image of a view over the screen: level 1 at the screen's size, a zoom scaled to ZOOM_SIDE square;
    1 px lines on the cell boundaries and a label at each cell's top-left (none with marks=False: a crop). Returns the
    image and each label's box."""
    x0, y0, w, h = view.window
    out_w, out_h = (w, h) if view.level == 1 else (ZOOM_SIDE, ZOOM_SIDE)
    source = screen.rgb
    columns = [((x0 + (i * w) // out_w) * 3) for i in range(out_w)]
    rgb = bytearray()
    for j in range(out_h):
        row = (y0 + (j * h) // out_h) * screen.width * 3
        rgb += b"".join(source[row + c:row + c + 3] for c in columns)
    image = pngio.Image(out_w, out_h, rgb)
    if not marks:
        return image, {}
    scale_x, scale_y = out_w / w, out_h / h
    for col in range(1, view.cols):
        fill(image, round(col * view.cell * scale_x), 0, 1, out_h, LINE)
    for row in range(1, view.rows):
        fill(image, 0, round(row * view.cell * scale_y), out_w, 1, LINE)
    cell_px = min(view.cell * scale_x, view.cell * scale_y)
    longest = max(len(name) for name in labels(view))
    glyph = max(1, min(3, int((cell_px - 4) // (4 * longest + 1))))
    boxes = {}
    for name in labels(view):
        col, row = parse(view, name)
        left, top = math.ceil(col * view.cell * scale_x) + 1, math.ceil(row * view.cell * scale_y) + 1
        width, height = glyph * (4 * len(name) - 1) + 2 * glyph, 7 * glyph
        fill(image, left, top, width, height, LABEL_FILLS[col % 2])
        for index, character in enumerate(name):
            for fy, line in enumerate(FONT[character]):
                for fx, bit in enumerate(line):
                    if bit == "1":
                        fill(image, left + glyph + (index * 4 + fx) * glyph, top + glyph + fy * glyph, glyph, glyph,
                             LABEL_INK)
        boxes[name] = (left, top, width, height)
    return image, boxes


def fill(image, x, y, w, h, colour):
    x1, y1 = max(0, x), max(0, y)
    x2, y2 = min(image.width, x + w), min(image.height, y + h)
    if x2 <= x1 or y2 <= y1:
        return
    span = bytes(colour) * (x2 - x1)
    for row in range(y1, y2):
        at = (row * image.width + x1) * 3
        image.rgb[at:at + len(span)] = span


def column_range(count):
    letters = COLUMNS[:count]
    skipped = [c for c in "IO" if letters[0] < c < letters[-1]]
    return f"A to {letters[-1]}" + (f" (no {' or '.join(skipped)})" if skipped else "")


def words(view, variant):
    """The result text for a grid image, in the design's words (the tool has none yet)."""
    if view.level == 1:
        text = (f"Grid {view.screen[0]}×{view.screen[1]}: {view.cols} columns {column_range(view.cols)}, {view.rows} "
                f"rows 1 to {view.rows}, each cell {view.cell} px. Call grid with the cell holding what you want")
        return text + (" to see it closer." if variant in CROPS else ", or click a cell.")
    factor = f"{ZOOM_SIDE / view.window[2]:.1f}".removesuffix(".0")
    named = " of ".join(reversed(view.path))
    if variant in CROPS:
        return (f"Cell {named}, {factor} times closer, with no grid. Click the middle of what you want with x and y "
                f"from 0 to {CROP_MAX} across this image, (0, 0) top-left and ({CROP_MAX}, {CROP_MAX}) bottom-right.")
    text = (f"Cell {named}, {factor} times closer: {view.cols} columns {column_range(view.cols)}, {view.rows} rows 1 "
            f"to {view.rows}, each cell {view.cell} px on the screen. Click the cell holding the middle of what you "
            "want.")
    if view.level <= len(VARIANTS[variant]):
        text += " If what you want is smaller than a cell, call grid with that cell to look closer once more."
    return text


def crop_spec(recorded):
    """The coordinates route's recorded `computer` spec for a crop variant: `grid` with `cell` for level 1, and x and
    y from 0 to 1000 across the latest grid image instead of the screenshot's pixels."""
    spec = copy.deepcopy(recorded)
    params = spec["parameters"]
    actions = params["properties"]["action"]["enum"]
    actions.insert(actions.index("screenshot") + 1 if "screenshot" in actions else 0, "grid")
    for axis in ("x", "y"):
        field = params["properties"][axis]
        if "in the latest screenshot's pixels" not in field.get("description", ""):
            raise ValueError("the recorded spec's x and y words changed: update crop_spec")
        field["description"] = field["description"].replace(
            "in the latest screenshot's pixels", f"from 0 to {CROP_MAX} across the latest grid image")
        field["maximum"] = CROP_MAX
    params["properties"]["cell"] = {
        "type": ["string", "null"],
        "description": "A cell's label from the latest grid, such as C4: for grid, to see that cell closer. Leave it "
                       "out to start a new grid."}
    params.setdefault("required", []).append("cell")
    spec["description"] = (spec.get("description", "").rstrip() + " To reach something on the screen that has no ref "
                           "or mark (a painted button, a drawn shape, a picture), call grid: the screen comes back "
                           "divided into labelled cells. Call grid with the cell holding the middle of what you want "
                           "to see it closer, then click it by x and y from 0 to 1000 across that closer image. Use a "
                           "ref or mark whenever there is one.")
    return spec


def tool_spec(recorded):
    """The mark route's recorded `computer` spec with `grid` and `cell`, in the design's words."""
    spec = copy.deepcopy(recorded)
    params = spec["parameters"]
    actions = params["properties"]["action"]["enum"]
    actions.insert(actions.index("screenshot") + 1 if "screenshot" in actions else 0, "grid")
    params["properties"]["cell"] = {
        "type": ["string", "null"],
        "description": "A cell's label from the latest grid, such as C4: for grid (to look closer), click, "
                       "double_click and scroll. Leave it out to start a new grid."}
    params.setdefault("required", []).append("cell")
    spec["description"] = (spec.get("description", "").rstrip() + " To reach something on the screen that has no ref "
                           "or mark (a painted button, a drawn shape, a picture), call grid: the screen comes back "
                           "divided into labelled cells. Call grid with the cell holding the middle of what you want "
                           "to see it closer, divided again, then click that finer cell by its label. Use a ref or "
                           "mark whenever there is one.")
    return spec
