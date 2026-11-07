"""Set-of-marks pictures for the grounding evaluation (c17, exploratory): the product's marks::draw
(crates/tqa-tools/src/builtin/computer/marks.rs) ported to Python, so marks the product never drew (a detector's
regions) look exactly like the ones it did; and the refs' boxes read back from a capture's marked screenshot, for a
capture that didn't record them.

    python3 marks.py refs CAPTURE OUT_DIR
    python3 marks.py build CAPTURE REFS_PROOF BOXES OUT_DIR --route NAME --kind merged|detector-only --threshold T
        --trigger note|screen

writes OUT_DIR/refs.json ({screen: [[x, y, width, height], ...]}, every marked ref's box in its number's order) and
OUT_DIR/refs-proof.json: per screen the marks with their numbers, the sha256 of its unmarked and marked PNGs, and the
pixels by which drawing those marks on the unmarked PNG differs from the product's marked one. It refuses (exit 1)
unless every screen differs by 0 pixels and has as many marks as its text says.

`build` writes a route's detector-marked screenshots for the runner's --marks (the final benchmark's B4) from
detect.py's BOXES at the operating THRESHOLD, the boxes rounded to whole pixels:
- merged: the product's refs (REFS_PROOF, from `refs`) and then the detector-marks note's regions (marks_merge: clear of
  every ref, the higher score of near repeats, at most 100 and never past 300 marks; the whole screen as the area),
  numbered after every ref of the snapshot, drawn in one call as the product would. With --trigger note, only where
  the note's triggers fire (section 2.1): "few", under 5 refs with a box; a screen where neither fires is today's,
  pixel for pixel and word for word. The capture records no opaque areas, so "opaque" is never taken to fire, and no
  focused window's box, so "few" keeps regions anywhere on the screen; --trigger screen adds regions on every screen;
- detector-only: every box at the threshold (at most 300, by score), numbered from e1 in reading order, as on a screen
  whose structure gives nothing.
OUT_DIR/marks.json holds the route, the kind, the threshold, the inputs' sha256, per screen the marks as the clicks are
scored from them ({"mark", "ref", "box"}: a region's ref is null, so only its centre can hit) and the shot (its PNG in
OUT_DIR and the tool's text with the note's words), and the sentence the tool's description gains.

Reading the boxes back: each mark's label is found by its number's exact pixels (the font, colours and sizes are the
product's), and its colour cycles with the marks drawn before it. Its box is then one whose label the product would have
put there (marks.rs `place`), whose outline shows the mark's colour wherever no later outline or label lies over it,
and which, of those, shows the most pixels of its colour no later mark already explains; marks are read from the last
drawn to the first. A last pass moves each box's edges by up to 2 px when that makes the drawing closer to the
product's: an edge lying under a later outline of the same colour can't be told apart otherwise."""

import hashlib
import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import marks_merge  # noqa: E402
import pngio  # noqa: E402

MAX_MARKS = 300
OUTLINE = 2
DIGIT_PIXELS = 10
IMAGE_SIDE = 1280  # computer.rs DEFAULT_IMAGE_SIDE, the capture's max_image_side
COLOURS = ((200, 30, 30), (30, 70, 200), (20, 120, 40), (150, 30, 150))
DIGIT = (255, 255, 255)
FONT = (0b111_101_101_101_111, 0b010_110_010_010_111, 0b111_001_111_100_111, 0b111_001_111_001_111,
        0b101_101_111_001_001, 0b111_100_111_001_111, 0b111_100_111_101_111, 0b111_001_010_010_010,
        0b111_101_111_101_111, 0b111_101_111_001_111)
COUNT = re.compile(r"^Screenshot \S+, (\d+) elements? marked")
REF = re.compile(r"^e(\d+) ", re.M)
KINDS = ("merged", "detector-only")
TRIGGERS = ("note", "screen")
# The detector-marks note's words (section 5.1): the result's first line, a region's line, and the tool's description.
REGIONS = ("Marks {first} to {last} are regions found in the picture, with no name: look at the image to see what "
           "each is.")
REGION = "Mark {first} is a region found in the picture, with no name: look at the image to see what it is."
REGION_LINE = "e{number} region (from the picture)"
NO_STRUCTURE = "Screenshot {size}, no element marked from the window's structure."
DESCRIPTION = ("Where the page or app can't describe part of the screen (a canvas, a picture, a remote screen), the "
               "screenshot also marks regions it found in the picture; click them by number the same way.")


# The port: raster.rs's Rect, fill and outline, and marks.rs's draw, unit, label_size and place.

def within(rect, width, height):
    """The part of a box inside the screen, or None."""
    x, y, w, h = rect
    left, top = max(x, 0), max(y, 0)
    right, bottom = min(x + w, width), min(y + h, height)
    return None if right <= left or bottom <= top else (left, top, right - left, bottom - top)


def overlaps(a, b):
    return a[0] < b[0] + b[2] and b[0] < a[0] + a[2] and a[1] < b[1] + b[3] and b[1] < a[1] + a[3]


def fill(image, rect, colour):
    rect = within(rect, image.width, image.height)
    if rect is None:
        return
    x, y, w, h = rect
    row = bytes(colour) * w
    for line in range(y, y + h):
        start = (line * image.width + x) * 3
        image.rgb[start:start + w * 3] = row


def outline(image, rect, thickness, colour):
    x, y, w, h = rect
    edge = min(thickness, w, h)
    for side in ((x, y, w, edge), (x, y + h - edge, w, edge), (x, y, edge, h), (x + w - edge, y, edge, h)):
        if side[2] and side[3]:
            fill(image, side, colour)


def unit(longest, max_side):
    """Screen pixels per font cell: a digit at least DIGIT_PIXELS high once scaled to max_side, and at least 2."""
    shown = longest if max_side == 0 else min(max_side, longest)
    return max(-(-(DIGIT_PIXELS * longest) // (5 * max(shown, 1))), 2)


def label_size(number, cell):
    return (len(str(number)) * 4 + 1) * cell, 7 * cell


def place(rect, size, screen, taken):
    """The label's place: above the box's top-left, else inside it, else below it, else above its top-right, each
    kept inside the screen; the first touching no earlier label, else the first moved down until clear."""
    w, h = size
    x, y, rw, rh = rect
    candidates = ((x, y - h), (x, y), (x, y + rh), (x + rw - w, y - h))

    def inside(at):
        return (min(max(at[0], 0), max(screen[0] - w, 0)), min(max(at[1], 0), max(screen[1] - h, 0)), w, h)

    def clear(label):
        return not any(overlaps(other, label) for other in taken)
    for at in candidates:
        if clear(inside(at)):
            return inside(at)
    first = inside(candidates[0])
    line = first[1]
    while line + h <= screen[1]:
        if clear(inside((first[0], line))):
            return inside((first[0], line))
        line += h
    return first


def draw(image, marks, max_side=IMAGE_SIDE):
    """Draws marks ([(number, [x, y, width, height])]) on image (pngio.Image with a bytearray) as the product does;
    returns the placed ones as (number, box, label, colour)."""
    cell = unit(max(image.width, image.height), max_side)
    placed = []
    for number, rect in marks[:MAX_MARKS]:
        rect = within(tuple(rect), image.width, image.height)
        if rect is None:
            continue
        colour = COLOURS[len(placed) % len(COLOURS)]
        label = place(rect, label_size(number, cell), (image.width, image.height), [p[2] for p in placed])
        placed.append((number, rect, label, colour))
    for _, rect, _, colour in placed:
        outline(image, rect, OUTLINE, colour)
    for number, _, label, colour in placed:
        fill(image, label, colour)
        digits(image, label, number, cell)
    return placed


def digits(image, label, number, cell):
    for index, digit in enumerate(str(number)):
        glyph = FONT[ord(digit) - ord("0")]
        left = label[0] + cell * (1 + 4 * index)
        for at in range(15):
            if glyph & (1 << (14 - at)):
                fill(image, (left + cell * (at % 3), label[1] + cell * (1 + at // 3), cell, cell), DIGIT)


def copy(image):
    return pngio.Image(image.width, image.height, bytearray(image.rgb))


def differing(a, b):
    """Pixels that differ between two images of one size."""
    row, count = a.width * 3, 0
    for start in range(0, len(a.rgb), row):
        if a.rgb[start:start + row] != b.rgb[start:start + row]:
            count += sum(a.rgb[i:i + 3] != b.rgb[i:i + 3] for i in range(start, start + row, 3))
    return count


# Reading the refs' boxes back from a marked screenshot.

def colour_at(image, x, y):
    if 0 <= x < image.width and 0 <= y < image.height:
        at = (y * image.width + x) * 3
        return tuple(image.rgb[at:at + 3])
    return None


def labels(marked, numbers, cell):
    """Each number's label as (number, box, colour), for the numbers drawn: a label's exact pixels, in the colour its
    place in the drawing order gives it (a number found inside a longer one's label isn't taken)."""
    width, height = marked.width, marked.height
    starts = {colour: [] for colour in COLOURS}
    for y in range(height):
        for x in range(width):
            colour = colour_at(marked, x, y)
            if colour in starts:
                starts[colour].append((x, y))
    out = []
    for number in numbers:
        colour = COLOURS[len(out) % len(COLOURS)]
        w, h = label_size(number, cell)
        template = pngio.Image(w, h, bytearray(bytes(colour) * w * h))
        digits(template, (0, 0, w, h), number, cell)
        rows = [bytes(template.rgb[r * w * 3:(r + 1) * w * 3]) for r in range(h)]
        for x, y in starts[colour]:
            if x + w <= width and y + h <= height and all(
                    bytes(marked.rgb[((y + r) * width + x) * 3:((y + r) * width + x + w) * 3]) == rows[r]
                    for r in range(h)):
                out.append((number, (x, y, w, h), colour))
                break
    return out


def edge_pixels(rect):
    x, y, w, h = rect
    edge = min(OUTLINE, w, h)
    return {(px, py) for sx, sy, sw, sh in ((x, y, w, edge), (x, y + h - edge, w, edge), (x, y, edge, h),
                                            (x + w - edge, y, edge, h))
            for py in range(sy, sy + sh) for px in range(sx, sx + sw)}


def candidates(marked, label, colour):
    """Boxes whose outline of colour starts or ends where the label's place puts a corner: an edge where a
    perpendicular edge of the colour starts, or where a run of it ends, or up to 2 px past that (under a later
    outline)."""
    width, height = marked.width, marked.height

    def shows(x, y):
        return colour_at(marked, x, y) == colour

    def grow(values, low, high):
        return sorted({v + d for v in values for d in (0, 1, 2) if low <= v + d <= high})

    def shrink(values):
        return sorted({v - d for v in values for d in (0, 1, 2) if v - d >= 0})

    def rights(x0, y):
        return grow([xr for xr in range(x0 + 1, width + 1) if shows(xr - 1, y) and (
            shows(xr - 1, y + 2) or shows(xr - 1, y - 2) or not shows(xr, y) or xr == width)], x0 + 1, width)

    def bottoms(x, y0):
        return grow([yb for yb in range(y0 + 1, height + 1) if shows(x, yb - 1) and (
            shows(x + 2, yb - 1) or shows(x - 2, yb - 1) or not shows(x, yb) or yb == height)], y0 + 1, height)
    lx, ly, lw, lh = label
    out = set()
    for x0, y0 in ((lx, ly + lh), (lx, ly)):        # the label above the box's top-left, or inside it
        left = bottoms(x0, y0)
        for xr in rights(x0, y0):
            for yb in set(left) | set(bottoms(xr - 1, y0)):
                out.add((x0, y0, xr - x0, yb - y0))
    tops = shrink([yt for yt in range(0, ly) if shows(lx, yt) and (shows(lx + 2, yt) or not shows(lx, yt - 1))])
    for yt in tops:                                  # the label below the box
        for xr in set(rights(lx, ly - 1)) | set(rights(lx, yt)):
            out.add((lx, yt, xr - lx, ly - yt))
    if ly == height - lh:                            # below a box reaching the screen's bottom, pushed back up
        corners = [yt for yt in range(0, ly) if shows(lx, yt) and shows(lx + 2, yt) and shows(lx, yt + 2)
                   and shows(lx + 1, yt + 1)]
        for yt in corners:
            for xr in rights(lx, yt):
                for yb in {ly + lh} | {yb for yb in bottoms(xr - 1, yt + 2) if yb > ly}:
                    out.add((lx, yt, xr - lx, yb - yt))
    xr, y0 = lx + lw, ly + lh                        # the label above the box's top-right
    lefts = shrink([xl for xl in range(0, xr) if shows(xl, y0) and (shows(xl, y0 + 2) or not shows(xl - 1, y0))])
    for xl in lefts:
        for yb in bottoms(xr - 1, y0):
            out.add((xl, y0, xr - xl, yb - y0))
    return sorted(box for box in out if box[2] > 0 and box[3] > 0)


def recover(plain, marked, text, max_side=IMAGE_SIDE):
    """The marks the product drew on this screen, as [(number, box)] in drawing order, and the pixels by which
    drawing them on plain differs from marked."""
    width, height = marked.width, marked.height
    cell = unit(max(width, height), max_side)
    numbers = sorted({int(n) for n in REF.findall(text)})
    found = labels(marked, numbers, cell)
    under_labels, taken, options = set(), [], []
    for number, label, colour in found:
        x, y, w, h = label
        under_labels.update((px, py) for py in range(y, y + h) for px in range(x, x + w))
        options.append([box for box in candidates(marked, label, colour)
                        if place(box, label[2:], (width, height), taken) == label])
        taken.append(label)
    # From the last drawn to the first: its outline shows its colour wherever nothing later covers it.
    covered = set(under_labels)
    pick = [None] * len(found)
    for j in range(len(found) - 1, -1, -1):
        colour = found[j][2]
        best = None
        for box in options[j]:
            pixels = edge_pixels(box)
            showing = {p for p in pixels if colour_at(marked, *p) == colour}
            if len(showing) + len(pixels & covered - showing) == len(pixels):
                key = (len(showing - covered), -box[2] * box[3])
                if best is None or key > best[0]:
                    best = (key, box, pixels)
        if best:
            pick[j] = best[1]
            covered |= best[2]
    pick = polish(plain, marked, found, pick, under_labels, (width, height))
    marks = [(found[j][0], list(pick[j])) for j in range(len(found)) if pick[j]]
    image = copy(plain)
    draw(image, marks, max_side)
    return marks, differing(image, marked), len(found)


def polish(plain, marked, found, pick, under_labels, screen):
    """Moves each box's edges by up to 2 px when the drawing gets closer to marked: outside the labels a pixel shows
    the colour of the last outline over it, else the unmarked screen's."""
    cover = {}

    def apply(j, box, add):
        for p in edge_pixels(box):
            owners = cover.setdefault(p, set())
            (owners.add if add else owners.discard)(j)
            if not owners:
                del cover[p]

    def right(p):
        if p in under_labels:
            return True
        owners = cover.get(p)
        return (found[max(owners)][2] if owners else colour_at(plain, *p)) == colour_at(marked, *p)
    for j, box in enumerate(pick):
        if box:
            apply(j, box, True)
    taken = [label for _, label, _ in found]
    for _ in range(4):
        changed = False
        for j, box in enumerate(pick):
            if not box:
                continue
            best, gain = box, 0
            x, y, w, h = box
            for dx in range(-2, 3):
                for dy in range(-2, 3):
                    for dw in range(-2, 3):
                        for dh in range(-2, 3):
                            other = (x + dx, y + dy, w + dw, h + dh)
                            if other == box or other[2] <= 0 or other[3] <= 0 or place(
                                    other, found[j][1][2:], screen, taken[:j]) != found[j][1]:
                                continue
                            changed_pixels = edge_pixels(box) ^ edge_pixels(other)
                            before = sum(right(p) for p in changed_pixels)
                            apply(j, box, False)
                            apply(j, other, True)
                            after = sum(right(p) for p in changed_pixels)
                            apply(j, other, False)
                            apply(j, box, True)
                            if after - before > gain:
                                best, gain = other, after - before
            if best != box:
                apply(j, box, False)
                apply(j, best, True)
                pick[j] = best
                changed = True
        if not changed:
            break
    return pick


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def refs(capture, out):
    """refs.json and refs-proof.json for a capture; False when any screen isn't reproduced exactly."""
    screens = json.loads((capture / "captures" / "captures.json").read_text(encoding="utf-8"))["screens"]
    boxes, proof, exact = {}, {}, True
    for screen in screens:
        plain_path, marked_path = capture / "captures" / screen["png"], capture / "captures" / screen["marked_png"]
        plain, marked = pngio.decode(plain_path.read_bytes()), pngio.decode(marked_path.read_bytes())
        plain, marked = copy(plain), copy(marked)
        said = COUNT.match(screen["text"])
        marks, differ, count = recover(plain, marked, screen["text"])
        ok = differ == 0 and said is not None and int(said.group(1)) == len(marks) == count
        exact = exact and ok
        boxes[screen["screen"]] = [box for _, box in marks]
        proof[screen["screen"]] = {"marks": [[number, box] for number, box in marks], "said": int(said.group(1))
                                   if said else None, "pixels_differing": differ, "png_sha256": sha256(plain_path),
                                   "marked_png_sha256": sha256(marked_path), "exact": ok}
        print(f"{screen['screen']}: {len(marks)} marks, {differ} pixels differ{'' if ok else '  NOT EXACT'}",
              flush=True)
    out.mkdir(parents=True, exist_ok=True)
    (out / "refs.json").write_text(json.dumps(boxes, indent=1, sort_keys=True) + "\n", encoding="utf-8")
    summary = {"screens": len(screens), "exact": sum(p["exact"] for p in proof.values()), "max_side": IMAGE_SIDE,
               "refs_json_sha256": sha256(out / "refs.json"), "per_screen": proof}
    (out / "refs-proof.json").write_text(json.dumps(summary, indent=1, sort_keys=True) + "\n", encoding="utf-8")
    print(f"{summary['exact']} of {len(screens)} screens exact; refs.json sha256 {summary['refs_json_sha256']}")
    return exact


# A route's detector-marked screenshots.

def whole(box):
    """A detector's box in whole screen pixels: its corners rounded."""
    left, top = round(box[0]), round(box[1])
    return [left, top, round(box[0] + box[2]) - left, round(box[1] + box[3]) - top]


def said(first, last):
    return REGION.format(first=first) if first == last else REGIONS.format(first=first, last=last)


def worded(text, regions, kind):
    """The screenshot's result text with the regions: the first line says which marks are regions, and each region's
    line follows the structure's in its block; detector-only drops the structure's lines and count."""
    first, rest = text.split("\n", 1)
    lines = [REGION_LINE.format(number=mark["number"]) for mark in regions]
    if kind == "detector-only":
        size = first.split(" ")[1].rstrip(",")
        first = NO_STRUCTURE.format(size=size)
        rest = re.sub(r", \d+ elements?\)", ", 0 elements)", rest, count=1)
        rest = "\n".join(line for line in rest.split("\n") if not REF.match(line + " "))
    if regions:
        first += " " + said(regions[0]["number"], regions[-1]["number"])
        closing = rest.rfind("</untrusted-content")
        rest = rest[:closing] + "".join(line + "\n" for line in lines) + rest[closing:]
    return first + "\n" + rest


def build(capture, proof_path, boxes_path, out, route, kind, threshold, trigger):
    if kind not in KINDS:
        sys.exit(f"--kind is one of {', '.join(KINDS)}")
    if trigger not in TRIGGERS:
        sys.exit(f"--trigger is one of {', '.join(TRIGGERS)}")
    proof = json.loads(proof_path.read_text(encoding="utf-8"))
    detected = json.loads(boxes_path.read_text(encoding="utf-8"))
    kept = detected.get("kept", detected.get("threshold", 0))
    if threshold < kept:
        sys.exit(f"--threshold {threshold} is below the {kept} the detector kept boxes from: rerun it lower")
    screens = json.loads((capture / "captures" / "captures.json").read_text(encoding="utf-8"))["screens"]
    made = {"route": route, "kind": kind, "threshold": threshold, "trigger": trigger, "model": detected.get("model"),
            "boxes_sha256": sha256(boxes_path), "refs_proof_sha256": sha256(proof_path), "description": DESCRIPTION,
            "screens": {}, "shots": {}}
    out.mkdir(parents=True, exist_ok=True)
    for screen in screens:
        name = screen["screen"]
        record = proof["per_screen"].get(name)
        plain_path = capture / "captures" / screen["png"]
        if not record or not record.get("exact") or record.get("png_sha256") != sha256(plain_path):
            sys.exit(f"{name}: its refs' proof is missing, not exact or for another PNG: run `refs` again")
        found = [(whole(box[:4]), box[4]) for box in detected["screens"].get(name, {}).get("boxes", [])
                 if box[4] >= threshold]
        found = [(box, score) for box, score in found if box[2] > 0 and box[3] > 0]
        numbers = sorted({int(n) for n in REF.findall(screen["text"])})
        ref_marks = [(number, box) for number, box in record["marks"]]
        if kind == "merged":
            # Every ref of the snapshot counts toward the 300; one with no box can't be overlapped.
            refs = [box for _, box in ref_marks] + [[0, 0, 0, 0]] * (len(numbers) - len(ref_marks))
            # The capture records no opaque areas: only "few" can fire.
            area = marks_merge.trigger([box for _, box in ref_marks]) if trigger == "note" else None
            regions = marks_merge.number(marks_merge.regions(found, refs, area), max(numbers, default=0))
            drawn = ref_marks + [(mark["number"], mark["box"]) for mark in regions]
            listed = [{"mark": number, "ref": f"e{number}", "box": box} for number, box in ref_marks]
        else:
            strongest = sorted(found, key=lambda d: -d[1])[:MAX_MARKS]
            regions = marks_merge.number([d for d in found if d in strongest], 0)
            drawn = [(mark["number"], mark["box"]) for mark in regions]
            listed = []
        listed += [{"mark": mark["number"], "ref": None, "box": mark["box"]} for mark in regions]
        image = copy(pngio.decode(plain_path.read_bytes()))
        draw(image, drawn)
        (out / f"{name}.png").write_bytes(pngio.encode(image))
        made["screens"][name] = listed
        made["shots"][name] = {"png": f"{name}.png", "text": worded(screen["text"], regions, kind)}
    (out / "marks.json").write_text(json.dumps(made, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"{route} ({kind}, threshold {threshold}): "
          f"{sum(m['ref'] is None for s in made['screens'].values() for m in s)} regions on {len(screens)} screens")
    return made


def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    if len(argv) == 3 and argv[0] == "refs":
        return 0 if refs(Path(argv[1]), Path(argv[2])) else 1
    if len(argv) == 13 and argv[0] == "build" and argv[5::2] == ["--route", "--kind", "--threshold", "--trigger"]:
        build(Path(argv[1]), Path(argv[2]), Path(argv[3]), Path(argv[4]), argv[6], argv[8], float(argv[10]),
              argv[12])
        return 0
    sys.exit(__doc__)


if __name__ == "__main__":
    sys.exit(main())
