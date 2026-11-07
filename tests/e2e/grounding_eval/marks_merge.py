"""Detector marks merged with the structure's refs (c17, exploratory), by the approved marks note's rule
(the detector-marks design, section 4.1 steps 3 to 6; section 4.2's numbers). One implementation for the final
benchmark's B4 clicks and its B7 count, standard library only.

Steps 1 and 2 (the detector's boxes at its operating threshold) are the caller's. `trigger` gives the area regions are
kept in, by the note's section 2.1: the whole screen when fewer than 5 refs have a box ("few"); else the opaque areas
(a canvas, a picture, an unread frame: what the structure can't see into) of at least 96 x 96 px with no ref centred
inside; else none, and the screenshot is today's. `regions` takes the boxes left and that area:
- 3. a box is kept only when its centre lies inside the trigger's area (any of its rectangles; None: the whole screen);
- 4. a box overlapping any ref at IoU 0.3 or more, or whose centre lies inside a ref's box, is dropped;
- 5. of two boxes left that overlap at IoU 0.5 or more, the lower score is dropped;
- 6. at most 100 are kept, and never more than leave room under the 300 marks a screenshot draws: lowest scores first.
The kept boxes keep the order they came in. `number` gives them the refs after the structure's, e(N+1) onward, in
reading order: by the box's top edge in bands of 16 px, then left to right.

Boxes are [x, y, width, height] in screen pixels; a detection is (box, score)."""

MAX_MARKS = 300
MAX_REGIONS = 100
REF_IOU = 0.3
SAME_IOU = 0.5
BAND = 16
FEW = 5
OPAQUE_SIDE = 96


def iou(a, b):
    """Intersection over union of two boxes; an empty box overlaps nothing."""
    left, top = max(a[0], b[0]), max(a[1], b[1])
    right, bottom = min(a[0] + a[2], b[0] + b[2]), min(a[1] + a[3], b[1] + b[3])
    if right <= left or bottom <= top:
        return 0.0
    shared = (right - left) * (bottom - top)
    return shared / (a[2] * a[3] + b[2] * b[3] - shared)


def centre_in(box, area):
    """Whether the box's centre lies inside the area: its near edges in, its far edges out, as the scorer counts."""
    x, y = box[0] + box[2] / 2, box[1] + box[3] / 2
    return area[0] <= x < area[0] + area[2] and area[1] <= y < area[1] + area[3]


def trigger(refs, opaque=()):
    """The area the note's triggers keep regions in: None (the whole screen) when fewer than FEW refs have a box; else
    the opaque areas at least OPAQUE_SIDE on each side with no ref's centre inside; else [] (no regions)."""
    if len(refs) < FEW:
        return None
    return [list(area) for area in opaque if area[2] >= OPAQUE_SIDE and area[3] >= OPAQUE_SIDE
            and not any(centre_in(ref, area) for ref in refs)]


def regions(detections, refs, area=None):
    """The detections a merged mark set adds, by steps 3 to 6, in the order given."""
    kept = [(box, score) for box, score in detections if area is None or any(centre_in(box, a) for a in area)]
    kept = [(box, score) for box, score in kept
            if not any(iou(box, ref) >= REF_IOU or centre_in(box, ref) for ref in refs)]
    by_score = sorted(range(len(kept)), key=lambda i: -kept[i][1])
    alive = set()
    for i in by_score:
        if not any(iou(kept[i][0], kept[j][0]) >= SAME_IOU for j in alive):
            alive.add(i)
    room = max(0, min(MAX_REGIONS, MAX_MARKS - len(refs)))
    alive = set(sorted(alive, key=lambda i: -kept[i][1])[:room])
    return [kept[i] for i in range(len(kept)) if i in alive]


def number(kept, refs_count):
    """The kept regions as marks after the refs: {"ref", "number", "box", "score"} in reading order (16 px bands by
    the top edge, then left to right; then the top edge, then the higher score)."""
    order = sorted(kept, key=lambda d: (d[0][1] // BAND, d[0][0], d[0][1], -d[1]))
    return [{"ref": f"e{refs_count + index}", "number": refs_count + index, "box": list(box), "score": score}
            for index, (box, score) in enumerate(order, start=1)]
