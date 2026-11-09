"""The grounding evaluation's scorer (docs/evals/computer-grounding.md section 4). Pure: it reads the target set, the
capture's ground truth and the runner's answers, and writes the numbers.

- targets.json: the targets (targets.py).
- captures.json: each target's ref and box on the screen, from the product's own snapshot (the capture).
- answers.jsonl: one line per target, way and route: {"route", "way", "target"} with, by way:
  - "coordinates": "x" and "y" in the image's pixels, and "image" [width, height] when the image sent was scaled; or,
    with "coordinate_space": "1000" (exploratory, c17), 0 to 1000 across the screenshot with "image" [1000, 1000],
    where 1000 is the right or bottom edge and counts as 999;
  - "mark" or "ref": "ref", as the model named it ("e7" or the mark's bare "7").
  A line may carry "error" instead (no call, an unparsable one): a miss. A target with no line is a miss too.

A canvas target (one with an "anchor", no element of its own) counts only on the coordinates way: ref and mark are
scored over the other targets, as the runner asks them.

A hit: the point falls inside the target's box (mapped to the screen as the tool maps it), or the ref names the
target's element. The recommendation was decided on 5 Oct 2026: `pixel_grounding` only when the
coordinates' Wilson 95 % lower bound is at least 90 % over the whole set.
"""

import json
import math
import sys
from pathlib import Path

WAYS = ("ref", "mark", "coordinates")
BAR = 0.90
Z = 1.96


def wilson(hits, total, z=Z):
    """A Wilson score interval of a hit rate; (None, None) without attempts."""
    if total == 0:
        return (None, None)
    p = hits / total
    centre = (p + z * z / (2 * total)) / (1 + z * z / total)
    half = z * math.sqrt(p * (1 - p) / total + z * z / (4 * total * total)) / (1 + z * z / total)
    return (max(0.0, centre - half), min(1.0, centre + half))


def needed(total, bar=BAR):
    """The fewest hits of `total` whose lower bound reaches the bar, or None when even all of them don't."""
    return next((hits for hits in range(total + 1) if wilson(hits, total)[0] >= bar), None)


def on_screen(value, image_side, screen_side):
    """An image pixel's centre on the screen, as the `computer` tool maps a point (ADR-0039)."""
    return (2 * value + 1) * screen_side // (2 * image_side)


def hit(answer, truth, screen):
    """Whether one answer hits its target's ground truth ({"ref", "box"})."""
    if answer is None or "error" in answer:
        return False
    if answer["way"] == "coordinates":
        x, y = answer.get("x"), answer.get("y")
        if not isinstance(x, int) or not isinstance(y, int) or x < 0 or y < 0:
            return False
        width, height = answer.get("image") or screen
        if answer.get("coordinate_space") == "1000":
            # (1000, 1000) is the bottom-right corner, inside the screenshot: the last cell's.
            if x > width or y > height:
                return False
            x, y = min(x, width - 1), min(y, height - 1)
        if x >= width or y >= height:
            return False
        x, y = on_screen(x, width, screen[0]), on_screen(y, height, screen[1])
        left, top, box_width, box_height = truth["box"]
        return left <= x < left + box_width and top <= y < top + box_height
    named = str(answer.get("ref", ""))
    if named.isdigit():
        named = "e" + named
    return named == truth["ref"]


def score(targets, truths, answers, screen):
    """Per route and way: hits, total, rate, interval, and the same per kind; and each route's recommendation."""
    by_key = {(answer["route"], answer["way"], answer["target"]): answer for answer in answers}
    routes = sorted({answer["route"] for answer in answers})
    report = {}
    for route in routes:
        ways = {}
        for way in WAYS:
            # A canvas target (no element of its own) is asked by coordinates only, so ref and mark count the others.
            asked = [target for target in targets if way == "coordinates" or "anchor" not in target]
            rows = [(target, hit(by_key.get((route, way, target["id"])), truths[target["id"]], screen))
                    for target in asked]
            ways[way] = summary(rows)
            ways[way]["kinds"] = {kind: summary([row for row in rows if row[0]["kind"] == kind])
                                  for kind in sorted({target["kind"] for target in targets})}
        low = ways["coordinates"]["low"]
        ways["pixel_grounding"] = low is not None and low >= BAR and ways["coordinates"]["total"] == len(targets)
        report[route] = ways
    return report


def summary(rows):
    hits = sum(1 for _, was_hit in rows if was_hit)
    total = len(rows)
    low, high = wilson(hits, total)
    return {"hits": hits, "total": total, "rate": hits / total if total else None, "low": low, "high": high}


def percent(value):
    return "n/a" if value is None else f"{100 * value:.1f} %"


def markdown(report, total):
    """The report as the admin guide's table, then each route's breakdown by kind."""
    lines = [f"Bar: coordinates' Wilson 95 % lower bound at least {percent(BAR)}; with {total} targets that is "
             f"{needed(total)} hits or more.", "",
             "| Route | By ref | By mark | By coordinates | pixel_grounding |", "|---|---|---|---|---|"]
    for route, ways in report.items():
        cells = [f"{ways[way]['hits']}/{ways[way]['total']} ({percent(ways[way]['rate'])}, "
                 f"{percent(ways[way]['low'])} to {percent(ways[way]['high'])})" for way in WAYS]
        lines.append(f"| {route} | " + " | ".join(cells) + f" | {'yes' if ways['pixel_grounding'] else 'no'} |")
    for route, ways in report.items():
        lines += ["", f"**{route}** by kind (hits/total):", "", "| Kind | " + " | ".join(WAYS) + " |",
                  "|---|" + "---|" * len(WAYS)]
        for kind in ways["coordinates"]["kinds"]:
            cells = [f"{ways[way]['kinds'][kind]['hits']}/{ways[way]['kinds'][kind]['total']}" for way in WAYS]
            lines.append(f"| {kind} | " + " | ".join(cells) + " |")
    return "\n".join(lines) + "\n"


def main(directory):
    directory = Path(directory)
    targets_file = json.loads((directory / "targets.json").read_text(encoding="utf-8"))
    targets, screen = targets_file["targets"], tuple(targets_file["screen"])
    truths = json.loads((directory / "captures" / "captures.json").read_text(encoding="utf-8"))["boxes"]
    missing = [target["id"] for target in targets if target["id"] not in truths]
    if missing:
        sys.exit(f"no ground truth for {len(missing)} targets, the first {missing[0]}: capture again")
    answers = [json.loads(line) for line in (directory / "answers.jsonl").read_text(encoding="utf-8").splitlines()
               if line.strip()]
    report = score(targets, truths, answers, screen)
    (directory / "report.json").write_text(json.dumps(report, indent=1), encoding="utf-8")
    (directory / "report.md").write_text(markdown(report, len(targets)), encoding="utf-8")
    print(markdown(report, len(targets)))


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else ".")
