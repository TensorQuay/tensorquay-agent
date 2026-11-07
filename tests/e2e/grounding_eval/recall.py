"""The grounding evaluation's no-model detector recall (c17, exploratory): before any model is asked to pick a mark, how
many targets a detector's boxes can reach at all, which caps whatever marks drawn from them can score.

    python3 recall.py DIR BOXES.json [--threshold T]

DIR holds targets.json and captures/captures.json (the targets' boxes from the product's own snapshot, in screen
pixels); BOXES.json is detect.py's output for that capture's unmarked screens. A target is reached:
- by box, when a detector box overlaps it at IoU 0.5 or more;
- by centre, when a detector box's centre lies inside it (the near edges in, the far edges out, as the scorer's hit).
Only boxes scoring at least the threshold count: the model's own (its card's), unless --threshold says otherwise, and
never below what the run kept. Writes DIR/recall-<model>.json and .md: each way's hits with its Wilson interval, the
same per kind, each target's best IoU, and the boxes per screen and the detector's latency.
"""

import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import detect  # noqa: E402
import scorer  # noqa: E402

IOU_BAR = 0.5


def iou(a, b):
    """Intersection over union of two boxes given as [x, y, width, height]."""
    left, top = max(a[0], b[0]), max(a[1], b[1])
    right, bottom = min(a[0] + a[2], b[0] + b[2]), min(a[1] + a[3], b[1] + b[3])
    inter = max(0, right - left) * max(0, bottom - top)
    union = a[2] * a[3] + b[2] * b[3] - inter
    return inter / union if union > 0 and inter > 0 else 0.0


def centre_in(detected, target):
    """Whether the detected box's centre lies in the target's box (near edges in, far edges out)."""
    cx, cy = detected[0] + detected[2] / 2, detected[1] + detected[3] / 2
    return target[0] <= cx < target[0] + target[2] and target[1] <= cy < target[1] + target[3]


def score(targets, truths, detected, threshold=None):
    """Per target and per kind: reached by box and by centre, over the boxes scoring at least the threshold."""
    threshold = detected["threshold"] if threshold is None else threshold
    kept = detected.get("kept", detected["threshold"])
    if threshold < kept:
        sys.exit(f"--threshold {threshold} is below the {kept} the run kept boxes at: rerun the detector lower")
    screens = {name: [box[:4] for box in shot.get("boxes", []) if box[4] >= threshold]
               for name, shot in detected["screens"].items()}
    per_target, kinds = {}, {}
    for target in targets:
        box = truths[target["id"]]["box"]
        found = screens.get(target["page"], [])
        best = max((iou(other, box) for other in found), default=0.0)
        reached = {"iou": best >= IOU_BAR, "centre": any(centre_in(other, box) for other in found),
                   "best_iou": round(best, 3)}
        per_target[target["id"]] = reached
        kind = kinds.setdefault(target["kind"], {"iou": 0, "centre": 0, "total": 0})
        kind["iou"] += reached["iou"]
        kind["centre"] += reached["centre"]
        kind["total"] += 1
    total = len(targets)
    report = {"model": detected["model"], "threshold": threshold, "kinds": kinds, "targets": per_target,
              "boxes_per_screen": {name: len(boxes) for name, boxes in screens.items()},
              "latency_ms": detect.latency([shot["ms"] for shot in detected["screens"].values() if "ms" in shot])}
    for way in ("iou", "centre"):
        hits = sum(1 for reached in per_target.values() if reached[way])
        low, high = scorer.wilson(hits, total)
        report[way] = {"hits": hits, "total": total, "low": low, "high": high}
    return report


def markdown(report):
    percent = scorer.percent
    timing = report["latency_ms"]
    boxes = report["boxes_per_screen"]
    lines = [f"**{report['model']}** at threshold {report['threshold']}: by box (IoU >= {IOU_BAR}) "
             f"{report['iou']['hits']}/{report['iou']['total']} ({percent(report['iou']['low'])} to "
             f"{percent(report['iou']['high'])}), by centre {report['centre']['hits']}/{report['centre']['total']} "
             f"({percent(report['centre']['low'])} to {percent(report['centre']['high'])}).", "",
             f"Boxes per screen: {min(boxes.values(), default=0)} to {max(boxes.values(), default=0)} "
             f"({sum(boxes.values())} on {len(boxes)} screens). Latency per screen: p50 {timing['p50']} ms, "
             f"p95 {timing['p95']} ms ({timing['n']} screens).", "",
             "| Kind | By box | By centre |", "|---|---|---|"]
    for kind, counts in sorted(report["kinds"].items()):
        lines.append(f"| {kind} | {counts['iou']}/{counts['total']} | {counts['centre']}/{counts['total']} |")
    return "\n".join(lines) + "\n"


def main(argv=None):
    parser = argparse.ArgumentParser()
    parser.add_argument("directory")
    parser.add_argument("boxes")
    parser.add_argument("--threshold", type=float)
    args = parser.parse_args(argv)
    directory = Path(args.directory)
    targets = json.loads((directory / "targets.json").read_text(encoding="utf-8"))["targets"]
    truths = json.loads((directory / "captures" / "captures.json").read_text(encoding="utf-8"))["boxes"]
    detected = json.loads(Path(args.boxes).read_text(encoding="utf-8"))
    report = score(targets, truths, detected, args.threshold)
    name = detected["model"]
    (directory / f"recall-{name}.json").write_text(json.dumps(report, indent=1), encoding="utf-8")
    (directory / f"recall-{name}.md").write_text(markdown(report), encoding="utf-8")
    print(markdown(report))


if __name__ == "__main__":
    main()
