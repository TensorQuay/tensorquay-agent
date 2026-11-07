"""The grounding evaluation's detector runner, without torch or a model: the screens it reads from the capture, the
letterbox scale OmniParser v3 needs, boxes turned into x, y, width, height clamped to the screen, the latencies'
p50 and p95, and the file it writes.

Run: python3 -m unittest tests/e2e/grounding_eval/test_detect.py"""

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import detect  # noqa: E402


class Pure(unittest.TestCase):
    # TEST: TOOL-U-073
    def test_the_screens_are_the_captures_unmarked_pngs(self):
        directory = Path(tempfile.mkdtemp())
        (directory / "captures.json").write_text(json.dumps({"screens": [
            {"screen": "form-01", "png": "form-01.png", "marked_png": "form-01.marked.png", "width": 1280,
             "height": 800}]}))
        self.assertEqual(detect.screens(directory), [("form-01", directory / "form-01.png", 1280, 800)])

    # TEST: TOOL-U-073
    def test_the_letterbox_keeps_the_aspect_and_the_scale_maps_back(self):
        self.assertEqual(detect.letterbox(1280, 800, 1280), (1.0, 1280, 800))
        self.assertEqual(detect.letterbox(2560, 1600, 1280), (0.5, 1280, 800))
        scale, width, height = detect.letterbox(800, 1280, 1280)
        self.assertEqual((scale, width, height), (1.0, 800, 1280))

    # TEST: TOOL-U-073
    def test_boxes_become_x_y_width_height_clamped_to_the_screen(self):
        rows = detect.as_boxes([[10.4, 20.6, 110.4, 60.6], [-5, -5, 30, 30], [1270, 790, 1300, 820]],
                               [0.91234, 0.5, 0.06], 1280, 800)
        self.assertEqual(rows, [[10, 21, 100, 40, 0.912], [0, 0, 30, 30, 0.5], [1270, 790, 10, 10, 0.06]])
        self.assertEqual(detect.as_boxes([[50, 50, 50, 80]], [0.9], 1280, 800), [], "no width: dropped")

    # TEST: TOOL-U-073
    def test_latency_p50_and_p95(self):
        self.assertEqual(detect.latency([100, 300, 200]), {"p50": 200, "p95": 300, "n": 3})
        self.assertEqual(detect.latency(list(range(1, 101))), {"p50": 50, "p95": 95, "n": 100})
        self.assertEqual(detect.latency([]), {"p50": None, "p95": None, "n": 0})

    # TEST: TOOL-U-073
    def test_the_written_file_names_the_model_its_licences_and_threshold(self):
        record = detect.record("omniparser-v3", 0.05, {"form-01": {"boxes": [], "ms": 10}}, "abc123")
        self.assertEqual(record["model"], "omniparser-v3")
        self.assertEqual((record["kept"], record["threshold"]), (0.05, 0.05))
        self.assertEqual(record["weights_sha256"], "abc123")
        self.assertIn("never shipped", record["use"])
        self.assertEqual(set(record["licence"]), {"code", "weights"})
        self.assertEqual(detect.MODELS["ui-detr-1"]["kept"], 0.05, "kept low; recall reads at its own threshold")
        self.assertEqual(detect.MODELS["ui-detr-1"]["threshold"], 0.35, "the card's benchmark threshold")
        self.assertIn("G-F", detect.MODELS["ui-detr-1"]["use"])
        with self.assertRaises(SystemExit):
            detect.record("yolo-agpl", 0.1, {}, "x")


if __name__ == "__main__":
    unittest.main()
