"""Detector marks merged with the structure's refs, by the approved marks note's rule (the detector-marks design,
section 4.1 steps 3 to 6, and section 4.2's numbers): one implementation for the final benchmark's B4 clicks and its
B7 count.

Run: python3 -m unittest tests/e2e/grounding_eval/test_marks_merge.py"""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import marks_merge  # noqa: E402


def found(*boxes):
    """Detections as ([x, y, width, height], score), the scores falling in the order given."""
    return [(list(box), round(0.9 - 0.01 * index, 2)) for index, box in enumerate(boxes)]


class Geometry(unittest.TestCase):
    # TEST: TOOL-U-092
    def test_iou_and_centre_as_the_scorer_counts_them(self):
        self.assertEqual(marks_merge.iou([0, 0, 10, 10], [0, 0, 10, 10]), 1.0)
        self.assertEqual(marks_merge.iou([0, 0, 10, 10], [10, 0, 10, 10]), 0.0, "touching edges share nothing")
        self.assertAlmostEqual(marks_merge.iou([0, 0, 10, 10], [5, 0, 10, 10]), 50 / 150)
        self.assertEqual(marks_merge.iou([0, 0, 0, 10], [0, 0, 10, 10]), 0.0, "an empty box overlaps nothing")
        self.assertTrue(marks_merge.centre_in([90, 90, 20, 20], [100, 100, 10, 10]), "the near corner is in")
        self.assertFalse(marks_merge.centre_in([100, 100, 20, 20], [100, 100, 10, 10]), "the far corner is out")


class Regions(unittest.TestCase):
    # TEST: TOOL-U-092
    def test_with_no_area_every_box_clear_of_the_refs_is_kept_in_the_order_given(self):
        boxes = found([10, 10, 40, 40], [300, 10, 40, 40], [10, 300, 40, 40])
        self.assertEqual(marks_merge.regions(boxes, []), boxes)

    # TEST: TOOL-U-092
    def test_step_3_keeps_only_boxes_centred_inside_the_triggers_area(self):
        inside, straddling, outside = [110, 110, 40, 40], [180, 110, 60, 40], [400, 400, 40, 40]
        kept = marks_merge.regions(found(inside, straddling, outside), [], area=[[100, 100, 100, 100]])
        self.assertEqual([box for box, _ in kept], [inside], "the straddling box's centre (210, 130) is outside")
        kept = marks_merge.regions(found(inside, outside), [], area=[[100, 100, 100, 100], [380, 380, 100, 100]])
        self.assertEqual([box for box, _ in kept], [inside, outside], "any of several opaque areas")

    # TEST: TOOL-U-092
    def test_step_4_drops_a_box_over_a_ref_by_iou_or_by_its_centre(self):
        ref = [100, 100, 200, 40]
        over = [105, 102, 190, 36]       # IoU about 0.86
        icon_inside = [110, 110, 20, 20]  # IoU 0.05, but its centre lies in the button
        beside = [100, 150, 200, 40]     # clear of it
        kept = marks_merge.regions(found(over, icon_inside, beside), [ref])
        self.assertEqual([box for box, _ in kept], [beside])
        below = [0, 50, 100, 100]        # IoU 5,000 / 15,000 = 0.33, its centre (50, 100) on the ref's far edge: out
        self.assertEqual(marks_merge.regions(found(below), [[0, 0, 100, 100]]), [], "the IoU alone drops it")

    # TEST: TOOL-U-092
    def test_step_4_keeps_a_box_just_under_the_iou_bar_whose_centre_is_outside(self):
        ref = [0, 0, 100, 100]
        box = [60, 0, 100, 100]          # IoU 40 / 160 = 0.25; centre (110, 50) outside the ref
        self.assertEqual([b for b, _ in marks_merge.regions(found(box), [ref])], [box])

    # TEST: TOOL-U-092
    def test_step_5_drops_the_lower_score_of_two_kept_boxes_at_iou_one_half_or_more(self):
        high, low, apart = ([10, 10, 100, 100], 0.8), ([20, 10, 100, 100], 0.9 - 0.5), ([500, 10, 100, 100], 0.3)
        kept = marks_merge.regions([low, apart, high], [])
        self.assertEqual(kept, [apart, high], "IoU 90/110 = 0.82: the lower score goes, the rest keep their order")
        under = ([60, 10, 100, 100], 0.4)  # IoU 50/150 with high: both stay
        self.assertEqual(marks_merge.regions([high, under], []), [high, under])

    # TEST: TOOL-U-092
    def test_step_5_runs_after_step_4_so_a_dropped_box_removes_nothing(self):
        ref = [0, 0, 100, 100]
        on_ref = ([60, 0, 60, 100], 0.9)       # its centre (90, 50) lies in the ref: step 4 drops it
        neighbour = ([80, 0, 60, 100], 0.5)    # IoU 0.5 with on_ref; with the ref 0.14 and its centre (110, 50) out
        self.assertEqual(marks_merge.regions([on_ref, neighbour], [ref]), [neighbour])
        self.assertEqual(marks_merge.regions([on_ref, neighbour], []), [on_ref], "with no ref, step 5 drops it")

    # TEST: TOOL-U-092
    def test_step_6_keeps_at_most_100_dropping_the_lowest_scores(self):
        boxes = [([20 * (i % 60), 30 * (i // 60), 10, 10], i / 1000) for i in range(150)]
        kept = marks_merge.regions(boxes, [])
        self.assertEqual(len(kept), 100)
        self.assertEqual(min(score for _, score in kept), 50 / 1000, "the 50 lowest go")
        self.assertEqual(kept, [b for b in boxes if b[1] >= 50 / 1000], "the rest keep their order")

    # TEST: TOOL-U-092
    def test_step_6_never_takes_the_marks_past_300(self):
        boxes = [([20 * (i % 60), 30 * (i // 60), 10, 10], i / 1000) for i in range(150)]
        refs = [[1250, 4 * i, 2, 2] for i in range(250)]
        kept = marks_merge.regions(boxes, refs)
        self.assertEqual(len(kept), 50, "300 - 250 refs")
        self.assertEqual(min(score for _, score in kept), 100 / 1000)
        self.assertEqual(marks_merge.regions(boxes, refs * 2), [], "refs alone at or past the cap leave no room")


class Numbers(unittest.TestCase):
    # TEST: TOOL-U-092
    def test_regions_follow_the_refs_in_reading_order_by_16_px_bands_then_left_to_right(self):
        kept = [([500, 40, 20, 20], 0.9), ([100, 47, 20, 20], 0.8), ([300, 10, 20, 20], 0.7), ([50, 48, 20, 20], 0.6)]
        numbered = marks_merge.number(kept, 23)
        self.assertEqual([(m["ref"], m["box"]) for m in numbered],
                         [("e24", [300, 10, 20, 20]),   # band 0
                          ("e25", [100, 47, 20, 20]),   # band 2 (32-47), left of 500
                          ("e26", [500, 40, 20, 20]),
                          ("e27", [50, 48, 20, 20])])   # band 3 (48-63), though left of all
        self.assertEqual([m["number"] for m in numbered], [24, 25, 26, 27])
        self.assertEqual([m["score"] for m in numbered], [0.7, 0.8, 0.9, 0.6])

    # TEST: TOOL-U-092
    def test_with_no_refs_the_regions_are_e1_onward(self):
        self.assertEqual([m["ref"] for m in marks_merge.number(found([0, 0, 5, 5], [9, 0, 5, 5]), 0)], ["e1", "e2"])
        self.assertEqual(marks_merge.number([], 7), [])

    # TEST: TOOL-U-092
    def test_one_band_ties_go_by_top_edge_then_by_score(self):
        same_x = [([100, 20, 10, 10], 0.5), ([100, 17, 10, 10], 0.6), ([100, 20, 10, 10], 0.9)]
        self.assertEqual([(m["box"][1], m["score"]) for m in marks_merge.number(same_x, 0)],
                         [(17, 0.6), (20, 0.9), (20, 0.5)])


class Trigger(unittest.TestCase):
    """The note's section 2.1: regions only when the structure finds little. trigger() gives the area regions() keeps
    them in: None (the whole screen) for "few", the opaque areas' boxes for "opaque", [] (none) when neither fires.
    A ref is inside an opaque area when its centre lies in it, as the scorer counts a centre."""

    # TEST: TOOL-U-092
    def test_few_fires_under_5_refs_with_a_box_and_keeps_regions_anywhere(self):
        four = [[10 * i, 0, 8, 8] for i in range(4)]
        self.assertIsNone(marks_merge.trigger(four))
        self.assertIsNone(marks_merge.trigger([]), "an empty snapshot")
        self.assertEqual(marks_merge.trigger(four + [[50, 0, 8, 8]]), [], "5 refs: neither fires")

    # TEST: TOOL-U-092
    def test_opaque_fires_on_an_area_of_96_by_96_or_more_with_no_ref_inside(self):
        refs = [[10 * i, 0, 8, 8] for i in range(6)]
        canvas, small, thin = [100, 100, 400, 300], [600, 100, 95, 300], [700, 100, 300, 95]
        self.assertEqual(marks_merge.trigger(refs, opaque=[canvas, small, thin]), [canvas], "at least 96 x 96")
        holding = [100, 100, 400, 300]
        self.assertEqual(marks_merge.trigger(refs + [[200, 200, 20, 20]], opaque=[holding]), [],
                         "a listed element inside: the structure sees into it")
        self.assertEqual(marks_merge.trigger(refs + [[90, 90, 20, 20]], opaque=[holding]), [],
                         "inside by its centre (100, 100), on the area's near edge")
        self.assertEqual(marks_merge.trigger(refs + [[80, 80, 20, 20]], opaque=[holding]), [holding],
                         "a ref beside it, centre (90, 90), doesn't count")

    # TEST: TOOL-U-092
    def test_few_wins_over_opaque_and_the_area_goes_to_regions(self):
        canvas = [100, 100, 400, 300]
        self.assertIsNone(marks_merge.trigger([[0, 0, 8, 8]], opaque=[canvas]), "the whole screen holds the area")
        refs = [[10 * i, 0, 8, 8] for i in range(6)]
        boxes = found([150, 150, 40, 40], [700, 500, 40, 40])
        self.assertEqual(marks_merge.regions(boxes, refs, marks_merge.trigger(refs, opaque=[canvas])), boxes[:1])
        self.assertEqual(marks_merge.regions(boxes, refs, marks_merge.trigger(refs)), [], "no trigger, no regions")
        self.assertEqual(marks_merge.regions(boxes, refs[:4], marks_merge.trigger(refs[:4])), boxes)


if __name__ == "__main__":
    unittest.main()
