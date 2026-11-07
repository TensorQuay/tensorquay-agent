"""The grounding evaluation's no-model detector recall, without a detector: a target is reached by box when a detector
box overlaps it at IoU 0.5 or more, and by centre when a box's centre lies inside it; both per kind, with the boxes per
screen and the threshold the detector's scores are cut at.

Run: python3 -m unittest tests/e2e/grounding_eval/test_recall.py"""

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import recall  # noqa: E402

TARGETS = [{"id": "f-1", "kind": "form", "page": "form-01"}, {"id": "f-2", "kind": "form", "page": "form-01"},
           {"id": "i-1", "kind": "icon", "page": "icon-01"}]
TRUTHS = {"f-1": {"ref": "e1", "box": [100, 100, 200, 40]}, "f-2": {"ref": "e2", "box": [100, 200, 200, 40]},
          "i-1": {"ref": "e1", "box": [10, 10, 24, 24]}}


class Geometry(unittest.TestCase):
    # TEST: TOOL-U-073
    def test_iou_of_boxes_given_as_x_y_width_height(self):
        self.assertEqual(recall.iou([0, 0, 10, 10], [0, 0, 10, 10]), 1.0)
        self.assertEqual(recall.iou([0, 0, 10, 10], [10, 0, 10, 10]), 0.0, "touching edges share nothing")
        self.assertAlmostEqual(recall.iou([0, 0, 10, 10], [5, 0, 10, 10]), 50 / 150)
        self.assertEqual(recall.iou([0, 0, 0, 10], [0, 0, 10, 10]), 0.0, "an empty box overlaps nothing")

    # TEST: TOOL-U-073
    def test_a_centre_inside_counts_on_the_near_edges_not_the_far_ones(self):
        self.assertTrue(recall.centre_in([100, 100, 10, 10], [100, 100, 6, 6]), "centre (103, 103)")
        self.assertTrue(recall.centre_in([90, 90, 20, 20], [100, 100, 10, 10]), "centre (100, 100), the near corner")
        self.assertFalse(recall.centre_in([100, 100, 20, 20], [100, 100, 10, 10]), "centre (110, 110), the far one")


class Recall(unittest.TestCase):
    def boxes(self, screens):
        # The run kept boxes from 0.2 up; it reads at 0.3 unless told otherwise.
        return {"model": "fake", "kept": 0.2, "threshold": 0.3, "screens": screens}

    # TEST: TOOL-U-073
    def test_each_target_by_box_and_by_centre_per_kind_with_boxes_per_screen(self):
        detected = self.boxes({
            # f-1: the same box (IoU 1); f-2: a box 15 px lower (IoU 5000/11000, under 0.5), its centre inside.
            "form-01": {"boxes": [[100, 100, 200, 40, 0.9], [100, 215, 200, 40, 0.8], [600, 600, 30, 30, 0.2]]},
            # i-1: a box far off.
            "icon-01": {"boxes": [[500, 500, 24, 24, 0.9]]},
        })
        report = recall.score(TARGETS, TRUTHS, detected)
        self.assertEqual((report["iou"]["hits"], report["iou"]["total"]), (1, 3))
        self.assertEqual((report["centre"]["hits"], report["centre"]["total"]), (2, 3))
        self.assertEqual(report["kinds"]["form"], {"iou": 1, "centre": 2, "total": 2})
        self.assertEqual(report["kinds"]["icon"], {"iou": 0, "centre": 0, "total": 1})
        self.assertEqual(report["boxes_per_screen"], {"form-01": 2, "icon-01": 1}, "the 0.2 box is under 0.3")
        self.assertEqual(report["targets"]["f-2"], {"iou": False, "centre": True, "best_iou": 0.455})
        self.assertIsNotNone(report["centre"]["low"])

    # TEST: TOOL-U-073
    def test_the_threshold_cuts_the_scores_and_a_screen_without_boxes_reaches_nothing(self):
        detected = self.boxes({"form-01": {"boxes": [[100, 100, 200, 40, 0.25]]}})
        self.assertEqual(recall.score(TARGETS, TRUTHS, detected)["centre"]["hits"], 0, "0.25 is under 0.3")
        self.assertEqual(recall.score(TARGETS, TRUTHS, detected, threshold=0.2)["centre"]["hits"], 1)
        with self.assertRaises(SystemExit):
            recall.score(TARGETS, TRUTHS, detected, threshold=0.1)  # below what the run kept: unknowable

    # TEST: TOOL-U-073
    def test_the_report_files(self):
        out = Path(tempfile.mkdtemp())
        captures = out / "captures"
        captures.mkdir()
        (out / "targets.json").write_text(json.dumps({"targets": TARGETS}))
        (captures / "captures.json").write_text(json.dumps({"boxes": TRUTHS, "screens": []}))
        boxes = out / "boxes-fake.json"
        boxes.write_text(json.dumps(self.boxes({"form-01": {"boxes": [[100, 100, 200, 40, 0.9]], "ms": 120},
                                                "icon-01": {"boxes": [], "ms": 80}})))
        recall.main([str(out), str(boxes)])
        written = json.loads((out / "recall-fake.json").read_text())
        self.assertEqual(written["centre"]["hits"], 1)
        text = (out / "recall-fake.md").read_text()
        self.assertIn("| form | 1/2 | 1/2 |", text)
        self.assertIn("by centre 1/3", text)
        self.assertIn("p50 80 ms", text, "the lower of two, as detect.latency takes a percentile")


if __name__ == "__main__":
    unittest.main()
