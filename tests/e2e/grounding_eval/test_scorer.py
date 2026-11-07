"""The grounding evaluation's scorer without a run: the Wilson bound and the hits each set size needs, each way's hit
rule, the per-kind breakdown and the recommendation, on synthetic targets and answers.

Run: python3 -m unittest tests/e2e/grounding_eval/test_scorer.py"""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import scorer  # noqa: E402

SCREEN = (1280, 800)


def targets(count, kind="form"):
    return [{"id": f"t{index}", "kind": kind} for index in range(count)]


def truths(count):
    return {f"t{index}": {"ref": f"e{index + 1}", "box": [100, 100, 50, 20]} for index in range(count)}


def points(count, hits, route="r"):
    """`hits` answers inside the box, the rest outside."""
    return [{"route": route, "way": "coordinates", "target": f"t{index}",
             "x": 120 if index < hits else 10, "y": 110} for index in range(count)]


class Bar(unittest.TestCase):
    def test_the_hits_each_set_size_needs(self):
        self.assertEqual(scorer.needed(200), 189)
        self.assertEqual(scorer.needed(300), 281)
        self.assertEqual(scorer.needed(60), 59)
        self.assertGreaterEqual(scorer.wilson(189, 200)[0], 0.90)
        self.assertLess(scorer.wilson(188, 200)[0], 0.90)
        self.assertEqual(scorer.wilson(0, 0), (None, None))


class Hits(unittest.TestCase):
    truth = {"ref": "e7", "box": [100, 100, 50, 20]}

    def answer(self, **fields):
        return {"route": "r", "target": "t0", **fields}

    def test_a_point_hits_inside_the_box_and_misses_on_its_far_edges(self):
        inside = self.answer(way="coordinates", x=100, y=100)
        right = self.answer(way="coordinates", x=150, y=110)
        below = self.answer(way="coordinates", x=120, y=120)
        self.assertTrue(scorer.hit(inside, self.truth, SCREEN))
        self.assertFalse(scorer.hit(right, self.truth, SCREEN))
        self.assertFalse(scorer.hit(below, self.truth, SCREEN))

    def test_a_point_on_a_scaled_image_is_mapped_to_the_screen_as_the_tool_maps_it(self):
        # The 1280 x 800 screen sent as 640 x 400: image pixel (60, 52) is screen (121, 105), inside the box.
        scaled = self.answer(way="coordinates", x=60, y=52, image=[640, 400])
        self.assertTrue(scorer.hit(scaled, self.truth, SCREEN))
        outside = self.answer(way="coordinates", x=640, y=10, image=[640, 400])
        self.assertFalse(scorer.hit(outside, self.truth, SCREEN), "outside the image itself")

    # TEST: TOOL-U-069
    def test_a_0_to_1000_point_maps_across_the_screen_and_1000_is_its_last_pixel(self):
        # The exploratory 0-1000 run (c17): 0-1000 across the screenshot, (1000, 1000) its bottom-right corner, so
        # 1000 counts as 999.
        corner = {"ref": "e9", "box": [1270, 790, 10, 10]}
        edge = self.answer(way="coordinates", x=1000, y=1000, image=[1000, 1000], coordinate_space="1000")
        self.assertTrue(scorer.hit(edge, corner, SCREEN), "1000 is the last pixel, (1279, 799)")
        beyond = self.answer(way="coordinates", x=1001, y=1000, image=[1000, 1000], coordinate_space="1000")
        self.assertFalse(scorer.hit(beyond, corner, SCREEN))
        origin = self.answer(way="coordinates", x=0, y=0, image=[1000, 1000], coordinate_space="1000")
        self.assertTrue(scorer.hit(origin, {"ref": "e1", "box": [0, 0, 2, 2]}, SCREEN))
        # 500 is screen pixel 640 (as the tool would map it: the centre of the 0-1000 cell), 125 is 100.
        middle = self.answer(way="coordinates", x=500, y=125, image=[1000, 1000], coordinate_space="1000")
        self.assertTrue(scorer.hit(middle, {"ref": "e2", "box": [640, 100, 1, 1]}, SCREEN))
        self.assertEqual(scorer.on_screen(999, 1000, 1280), 1279)
        # Without the space, a 1000 x 1000 image is pixels: 1000 is outside it, as before.
        pixels = self.answer(way="coordinates", x=1000, y=1000, image=[1000, 1000])
        self.assertFalse(scorer.hit(pixels, corner, SCREEN))

    def test_a_ref_or_a_marks_bare_number_names_the_element(self):
        self.assertTrue(scorer.hit(self.answer(way="mark", ref="7"), self.truth, SCREEN))
        self.assertTrue(scorer.hit(self.answer(way="ref", ref="e7"), self.truth, SCREEN))
        self.assertFalse(scorer.hit(self.answer(way="mark", ref="e8"), self.truth, SCREEN))

    def test_an_error_a_bad_point_or_no_answer_is_a_miss(self):
        self.assertFalse(scorer.hit(None, self.truth, SCREEN))
        self.assertFalse(scorer.hit(self.answer(way="coordinates", error="no call"), self.truth, SCREEN))
        self.assertFalse(scorer.hit(self.answer(way="coordinates", x="120", y=110), self.truth, SCREEN))
        self.assertFalse(scorer.hit(self.answer(way="coordinates", x=-1, y=110), self.truth, SCREEN))


class Report(unittest.TestCase):
    def test_the_recommendation_follows_the_bar_over_the_whole_set(self):
        good = scorer.score(targets(200), truths(200), points(200, 189), SCREEN)["r"]
        self.assertEqual(good["coordinates"]["hits"], 189)
        self.assertTrue(good["pixel_grounding"])
        short = scorer.score(targets(200), truths(200), points(200, 188), SCREEN)["r"]
        self.assertFalse(short["pixel_grounding"])
        missing = scorer.score(targets(200), truths(200), points(150, 150), SCREEN)["r"]
        self.assertEqual((missing["coordinates"]["hits"], missing["coordinates"]["total"]), (150, 200))
        self.assertFalse(missing["pixel_grounding"], "an unanswered target is a miss")

    # TEST: TOOL-U-072
    def test_ref_and_mark_are_scored_over_the_element_targets_and_coordinates_over_all(self):
        # A canvas target is asked only by coordinates, so it can't count as a ref or mark miss.
        canvas = [{"id": f"c{index}", "kind": "canvas", "anchor": {"role": "heading", "name": "T"}}
                  for index in range(3)]
        known = {**truths(4), **{f"c{index}": {"ref": None, "box": [100, 100, 50, 20]} for index in range(3)}}
        answers = points(4, 4) + [{"route": "r", "way": "coordinates", "target": f"c{index}", "x": 120, "y": 110}
                                  for index in range(3)]
        answers += [{"route": "r", "way": "ref", "target": f"t{index}", "ref": f"e{index + 1}"} for index in range(4)]
        report = scorer.score(targets(4) + canvas, known, answers, SCREEN)["r"]
        self.assertEqual((report["coordinates"]["hits"], report["coordinates"]["total"]), (7, 7))
        self.assertEqual((report["ref"]["hits"], report["ref"]["total"]), (4, 4))
        self.assertEqual(report["mark"]["total"], 4)
        self.assertEqual(report["coordinates"]["kinds"]["canvas"]["hits"], 3)
        self.assertEqual(report["ref"]["kinds"]["canvas"]["total"], 0)

    def test_each_way_is_broken_down_by_kind(self):
        mixed = targets(4, "form") + [{"id": "t4", "kind": "icon"}, {"id": "t5", "kind": "icon"}]
        report = scorer.score(mixed, truths(6), points(6, 5), SCREEN)["r"]
        self.assertEqual(report["coordinates"]["kinds"]["form"]["hits"], 4)
        self.assertEqual(report["coordinates"]["kinds"]["icon"]["hits"], 1)
        self.assertEqual(report["mark"]["hits"], 0, "no mark answers: all misses")
        text = scorer.markdown({"r": report}, 6)
        self.assertIn("| icon | 0/2 | 0/2 | 1/2 |", text)


if __name__ == "__main__":
    unittest.main()
