"""The grounding evaluation's detector runner (c17, exploratory, never shipped): one UI element detector over a
capture's unmarked screens, writing every box it finds with its score and the time each screen took. recall.py reads
the output. Torch and the model's library are needed only here, in a throwaway container with no network and the
weights mounted read-only; the measurement's pure parts below need neither.

    python3 detect.py MODEL WEIGHTS CAPTURES_DIR OUT.json

MODEL is one of MODELS. Each keeps its boxes from a low score (`kept`) so recall can read at the card's own threshold
(`threshold`) or any higher one without a rerun. The licences are recorded in the output, and neither model ships:
UI-DETR-1 is measured while the owner's decision G-F (its training data) waits, and OmniParser v3 is a reference
only, since its weights come from GPL-3.0 YOLOv9-E ones.
"""

import hashlib
import json
import math
import sys
import time
from pathlib import Path

MODELS = {
    "ui-detr-1": {
        "source": "huggingface.co/racineai/UI-DETR-1 @ 0f0dda5ec55a9a873f2937dd755863df6218ef80, model.pth "
                  "(534,747,653 bytes)",
        "licence": {"code": "rfdetr 1.11.2, Apache-2.0 (with torch and torchvision, BSD-3-Clause)",
                    "weights": "MIT (its card); RF-DETR-M's COCO checkpoint, Apache-2.0, underneath; trained on six "
                               "Roboflow Universe datasets whose terms aren't verified (G-F)"},
        "use": "measurement only, while G-F waits on the owner",
        "kept": 0.05, "threshold": 0.35, "resolution": 1600,
    },
    "omniparser-v3": {
        "source": "huggingface.co/microsoft/OmniParser-v2.0 @ f55d0750e5b94db2125ef0b45b0fa4a85ddc59b4, "
                  "icon_detect_v3/model.pt (281,182,680 bytes), TorchScript",
        "licence": {"code": "torch and torchvision only (BSD-3-Clause); the decode is its card's, written here",
                    "weights": "MIT (icon_detect_v3/LICENSE), but fine-tuned from WongKinYiu's GPL-3.0 YOLOv9-E "
                               "weights, so outside the licence bar"},
        "use": "a reference only, never shipped",
        "kept": 0.05, "threshold": 0.05, "imgsz": 1280, "nms_iou": 0.45, "strides": (8, 16, 32),
    },
}


def screens(captures):
    """The capture's screens: (name, its unmarked PNG, width, height), in the capture's order."""
    listed = json.loads((Path(captures) / "captures.json").read_text(encoding="utf-8"))["screens"]
    return [(shot["screen"], Path(captures) / shot["png"], shot["width"], shot["height"]) for shot in listed]


def letterbox(width, height, side):
    """OmniParser v3's top-left letterbox into a side x side square: the scale, and the resized width and height."""
    scale = min(side / width, side / height)
    return scale, round(width * scale), round(height * scale)


def as_boxes(xyxy, scores, width, height):
    """[x, y, width, height, score] in whole screen pixels, clamped to the screen; a box with no area is dropped."""
    rows = []
    for (x1, y1, x2, y2), score in zip(xyxy, scores):
        x1, x2 = (round(min(max(value, 0), width)) for value in (x1, x2))
        y1, y2 = (round(min(max(value, 0), height)) for value in (y1, y2))
        if x2 > x1 and y2 > y1:
            rows.append([x1, y1, x2 - x1, y2 - y1, round(float(score), 3)])
    return rows


def latency(times):
    """p50 and p95 of a list of milliseconds (nearest rank)."""
    if not times:
        return {"p50": None, "p95": None, "n": 0}
    ordered = sorted(times)
    rank = lambda share: ordered[max(0, math.ceil(share * len(ordered)) - 1)]  # noqa: E731
    return {"p50": rank(0.50), "p95": rank(0.95), "n": len(ordered)}


def record(model, kept, shots, weights_sha256):
    """What the run writes: the model, its source and licences, the score kept from, and each screen's boxes."""
    if model not in MODELS:
        sys.exit(f"unknown model {model}: one of {', '.join(MODELS)}")
    spec = MODELS[model]
    return {"model": model, "source": spec["source"], "licence": spec["licence"], "use": spec["use"],
            "kept": kept, "threshold": spec["threshold"], "weights_sha256": weights_sha256, "screens": shots}


def ui_detr(weights):
    """UI-DETR-1 as its card runs it: RF-DETR-M at resolution 1600, predicting on the RGB screen."""
    from rfdetr.detr import RFDETRMedium

    model = RFDETRMedium(pretrain_weights=str(weights), resolution=MODELS["ui-detr-1"]["resolution"])

    def run(image):
        found = model.predict(image, threshold=MODELS["ui-detr-1"]["kept"])
        return found.xyxy.tolist(), found.confidence.tolist()
    return run


def omniparser_v3(weights):
    """OmniParser v3 as its card decodes it: letterbox to 1280, sigmoid class logits, ltrb distances per stride,
    the kept score, NMS at 0.45, then back to screen pixels."""
    import numpy
    import torch
    from PIL import Image
    from torchvision.ops import nms

    spec = MODELS["omniparser-v3"]
    model = torch.jit.load(str(weights), map_location="cpu").eval()

    def run(image):
        scale, width, height = letterbox(image.width, image.height, spec["imgsz"])
        canvas = Image.new("RGB", (spec["imgsz"], spec["imgsz"]), (114, 114, 114))
        canvas.paste(image.resize((width, height), Image.BILINEAR), (0, 0))
        tensor = torch.from_numpy(numpy.array(canvas)).permute(2, 0, 1).float()[None] / 255.0
        with torch.no_grad():
            outputs = model(tensor)
        boxes, scores = [], []
        for index, stride in enumerate(spec["strides"]):
            cls = outputs[2 * index].sigmoid()[0, 0]
            left, top, right, bottom = outputs[2 * index + 1][0]
            grid = cls.shape[-1]
            gy, gx = torch.meshgrid(torch.arange(grid, dtype=torch.float32),
                                    torch.arange(grid, dtype=torch.float32), indexing="ij")
            cx, cy = gx + 0.5, gy + 0.5
            boxes.append(torch.stack([(cx - left) * stride, (cy - top) * stride, (cx + right) * stride,
                                      (cy + bottom) * stride], dim=-1).reshape(-1, 4))
            scores.append(cls.reshape(-1))
        boxes, scores = torch.cat(boxes), torch.cat(scores)
        keep = scores > spec["kept"]
        boxes, scores = boxes[keep], scores[keep]
        keep = nms(boxes, scores, spec["nms_iou"])
        return (boxes[keep] / scale).tolist(), scores[keep].tolist()
    return run


def main(argv=None):
    model, weights, captures, out = (argv if argv is not None else sys.argv[1:])[:4]
    if model not in MODELS:
        sys.exit(f"unknown model {model}: one of {', '.join(MODELS)}")
    from PIL import Image

    digest = hashlib.sha256(Path(weights).read_bytes()).hexdigest()
    run = (ui_detr if model == "ui-detr-1" else omniparser_v3)(weights)
    shots = {}
    for name, png, width, height in screens(captures):
        image = Image.open(png).convert("RGB")
        started = time.perf_counter()
        xyxy, scores = run(image)
        elapsed = round((time.perf_counter() - started) * 1000)
        shots[name] = {"boxes": as_boxes(xyxy, scores, width, height), "ms": elapsed}
        print(f"{name}: {len(shots[name]['boxes'])} boxes, {elapsed} ms", file=sys.stderr, flush=True)
    Path(out).write_text(json.dumps(record(model, MODELS[model]["kept"], shots, digest), indent=1), encoding="utf-8")


if __name__ == "__main__":
    main()
