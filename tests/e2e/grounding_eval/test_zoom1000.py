"""Grid then zoom then 0-1000 (z1000) without a model: the crop round the chosen level-1 cell, its point mapped to the
screen as the scorer maps Arm 1's 0-1000, its drawing with no grid, its words and tool spec, and the runner's
conversation, refusals and answer line, which scorer.py scores unchanged.

Run: python3 -m unittest tests/e2e/grounding_eval/test_zoom1000.py"""

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import grid  # noqa: E402
import grid_runner  # noqa: E402
import pngio  # noqa: E402
import scorer  # noqa: E402

TARGET = {"id": "form-01-01", "kind": "form", "page": "form-01", "instruction": "Click the Email field."}
# The target: 40 x 20 at (485, 650): its middle (505, 660) is in level-1 cell D5 (480-640, 640-800).
TRUTH = {"form-01-01": {"ref": "e1", "box": [485, 650, 40, 20]}}
PIXELS = "in the latest screenshot's pixels"
AXIS = {"type": ["integer", "null"], "minimum": 0, "maximum": 8192,
        "description": f"For click: the point's x {PIXELS}, instead of a ref"}
MARK_SPEC = {"name": "computer", "description": "Operate the desktop.",
             "parameters": {"type": "object", "additionalProperties": False, "required": ["action", "ref"],
                            "properties": {"action": {"type": "string", "enum": ["screenshot", "click"]},
                                           "ref": {"type": ["string", "null"]}}}}
COORDINATES_SPEC = {"name": "computer", "description": "Operate the desktop.",
                    "parameters": {"type": "object", "additionalProperties": False,
                                   "required": ["action", "ref", "x", "y"],
                                   "properties": {"action": {"type": "string", "enum": ["screenshot", "click"]},
                                                  "ref": {"type": ["string", "null"]}, "x": dict(AXIS),
                                                  "y": dict(AXIS, description=f"For click: the point's y {PIXELS}, "
                                                                                  "instead of a ref")}}}


def call(arguments, finish="tool_calls"):
    return {"choices": [{"finish_reason": finish, "message": {"tool_calls": [
        {"function": {"name": "computer", "arguments": json.dumps(arguments)}}]}}],
        "usage": {"prompt_tokens": 1500, "completion_tokens": 100}}


def level1():
    return grid.level1(1280, 800)


class Geometry(unittest.TestCase):
    # TEST: TOOL-U-075
    def test_the_crop_is_the_cell_plus_20_px_shifted_inside_the_screen_from_level_1_only(self):
        d5 = grid.zoom(level1(), "D5", "z1000")
        # D5 is x 480-640, y 640-800: plus 20 px is (460, 620, 200, 200), shifted up to end at the screen's bottom.
        self.assertEqual((d5.window, d5.cell, d5.level, d5.path), ((460, 600, 200, 200), 200, 2, ["D5"]))
        self.assertEqual((d5.cols, d5.rows), (1, 1), "one cell: no grid on the crop")
        self.assertEqual(grid.zoom(level1(), "a1", "z1000").window, (0, 0, 200, 200))
        self.assertIsNone(grid.zoom(d5, "A1", "z1000"), "no zoom from the crop")
        self.assertIsNone(grid.zoom(level1(), "Z9", "z1000"))

    # TEST: TOOL-U-075
    def test_a_0_to_1000_point_maps_to_the_crops_pixels_as_the_scorer_maps_arm_1(self):
        d5 = grid.zoom(level1(), "D5", "z1000")
        self.assertEqual(grid.crop_point(d5, 0, 0), (460, 600))
        self.assertEqual(grid.crop_point(d5, 500, 500), (560, 700))
        self.assertEqual(grid.crop_point(d5, 1000, 1000), (659, 799), "1000 is the far edge, counted as 999")
        self.assertEqual(grid.crop_point(d5, 225, 300), (505, 660))
        for x, y in [(-1, 0), (0, 1001), (5.0, 5), (True, 5), (None, 5), ("5", 5)]:
            self.assertIsNone(grid.crop_point(d5, x, y), (x, y))
        # The same mapping as the scorer's for a 0-1000 answer over a 200 px image (scorer.on_screen).
        for value in (0, 1, 333, 500, 999):
            self.assertEqual(grid.crop_point(d5, value, 0)[0] - 460, scorer.on_screen(value, 1000, 200))

    # TEST: TOOL-U-075
    def test_the_crop_is_drawn_at_640_with_no_lines_or_labels(self):
        screen = pngio.Image(1280, 800, bytearray([250, 250, 250] * 1280 * 800))
        grid.fill(screen, 500, 650, 20, 20, (200, 0, 0))
        d5 = grid.zoom(level1(), "D5", "z1000")
        image, labels = grid.render(screen, d5, marks=False)
        self.assertEqual((image.width, image.height, labels), (640, 640, {}))
        colours = {bytes(image.rgb[i:i + 3]) for i in range(0, len(image.rgb), 3)}
        self.assertEqual(colours, {bytes([250, 250, 250]), bytes([200, 0, 0])}, "no grid line or label colour")
        # Screen (510, 660) is 3.2 times (50, 60) into the crop.
        at = (192 * 640 + 160) * 3
        self.assertEqual(bytes(image.rgb[at:at + 3]), bytes([200, 0, 0]))

    # TEST: TOOL-U-075
    def test_the_words_and_the_spec_ask_0_to_1000_on_the_crop(self):
        self.assertTrue(grid.words(level1(), "z1000").endswith("Call grid with the cell holding what you want to see "
                                                               "it closer."))
        self.assertEqual(grid.words(grid.zoom(level1(), "D5", "z1000"), "z1000"),
                         "Cell D5, 3.2 times closer, with no grid. Click the middle of what you want with x and y from "
                         "0 to 1000 across this image, (0, 0) top-left and (1000, 1000) bottom-right.")
        spec = grid.crop_spec(COORDINATES_SPEC)
        fields = spec["parameters"]["properties"]
        self.assertEqual(fields["action"]["enum"], ["screenshot", "grid", "click"])
        for axis in ("x", "y"):
            self.assertEqual(fields[axis]["maximum"], 1000)
            self.assertIn("from 0 to 1000 across the latest grid image", fields[axis]["description"])
            self.assertNotIn(PIXELS, fields[axis]["description"])
        self.assertIn("cell", spec["parameters"]["required"])
        self.assertEqual(COORDINATES_SPEC["parameters"]["properties"]["x"]["maximum"], 8192, "the record is unchanged")
        changed = json.loads(json.dumps(COORDINATES_SPEC).replace(PIXELS, "in pixels"))
        with self.assertRaises(ValueError):
            grid.crop_spec(changed)


class Setup(unittest.TestCase):
    def setUp(self):
        self.directory = Path(tempfile.mkdtemp())
        captures = self.directory / "captures"
        captures.mkdir()
        screen = pngio.Image(1280, 800, bytearray([250, 250, 250] * 1280 * 800))
        (captures / "form-01.png").write_bytes(pngio.encode(screen))
        (captures / "captures.json").write_text(json.dumps({
            "screens": [{"screen": "form-01", "png": "form-01.png", "width": 1280, "height": 800}],
            "boxes": TRUTH, "tools": {"mark": MARK_SPEC, "coordinates": COORDINATES_SPEC}}))
        (self.directory / "targets.json").write_text(json.dumps({"screen": [1280, 800], "targets": [TARGET]}))
        self.sent = []

    def replies(self, *answers):
        queue = list(answers)

        def post(base_url, model, body, key=None):
            self.sent.append(body)
            return queue.pop(0)
        return post

    def run_one(self, *answers):
        job = grid_runner.Job(self.directory, "z1000", grid_runner.load(self.directory))
        return job.answer(TARGET, self.replies(*answers), "http://m", "m", None, 16384, {})


class Turns(Setup):
    # TEST: TOOL-U-075
    def test_level_1_then_the_crop_then_a_0_to_1000_click_scored_unchanged(self):
        line = self.run_one(call({"action": "grid", "cell": "D5"}), call({"action": "click", "x": 225, "y": 300}))
        self.assertEqual((line["x"], line["y"], line["image"]), (505, 660, [1280, 800]))
        self.assertEqual((line["crop_x"], line["crop_y"], line["crop_window"]), (225, 300, [460, 600, 200, 200]))
        self.assertEqual((line["way"], line["variant"], line["cells"], line["calls"]),
                         ("coordinates", "z1000", ["D5"], 2))
        self.assertEqual([level["holds_target"] for level in line["levels"]], [True, True])
        self.assertTrue(scorer.hit(line, TRUTH["form-01-01"], [1280, 800]), "the scorer reads it as screen pixels")
        first, second = self.sent
        self.assertEqual(first["tools"][0]["function"]["parameters"]["properties"]["x"]["maximum"], 1000)
        self.assertTrue(first["messages"][3]["content"].endswith("to see it closer."))
        self.assertTrue(second["messages"][-2]["content"].startswith("Cell D5, 3.2 times closer, with no grid."))
        self.assertEqual(sum(1 for m in second["messages"] if isinstance(m["content"], list)), 2, "two images")
        cached = self.directory / "grid-cache" / "z1000" / "form-01" / "D5.png"
        self.assertEqual(pngio.decode(cached.read_bytes()).width, 640)
        # A click just outside the target misses, and the scorer agrees.
        line = self.run_one(call({"action": "grid", "cell": "D5"}), call({"action": "click", "x": 120, "y": 300}))
        self.assertEqual(line["levels"][-1]["holds_target"], False)
        self.assertFalse(scorer.hit(line, TRUTH["form-01-01"], [1280, 800]))

    # TEST: TOOL-U-075
    def test_the_refusals_are_misses_that_say_why(self):
        on_crop = [
            (call({"action": "click", "cell": "A1", "x": 225, "y": 300}), "a cell on the crop"),
            (call({"action": "click", "x": 1001, "y": 300}), "a point outside 0 to 1000"),
            (call({"action": "click", "x": 22.5, "y": 300}), "a point outside 0 to 1000"),
            (call({"action": "click", "ref": "e1"}), "no point"),
            (call({"action": "grid", "cell": "A1"}), "action grid on the crop"),
        ]
        for reply, error in on_crop:
            self.sent.clear()
            line = self.run_one(call({"action": "grid", "cell": "D5"}), reply)
            self.assertEqual((line["error"], len(self.sent)), (error, 2), error)
        # On level 1, as the grid variants: no coordinates, a cell that exists, and a pick that holds the target.
        self.assertEqual(self.run_one(call({"action": "click", "x": 500, "y": 660}))["error"], "wrote coordinates")
        self.assertEqual(self.run_one(call({"action": "grid", "cell": "Z9"}))["error"], "no such cell")
        self.sent.clear()
        line = self.run_one(call({"action": "grid", "cell": "A1"}))
        self.assertEqual((line["error"], len(self.sent)), ("picked a cell whose window can't hold the target", 1))


class Run(Setup):
    # TEST: TOOL-U-075
    def test_a_run_writes_one_line_and_resumes_and_needs_the_coordinates_spec(self):
        saved = grid_runner.runner.post
        grid_runner.runner.post = self.replies(call({"action": "grid", "cell": "D5"}),
                                               call({"action": "click", "x": 225, "y": 300}))
        try:
            args = [str(self.directory), "--variant", "z1000", "--route", "r-z1000", "--base-url", "http://m",
                    "--model", "m"]
            grid_runner.main(args)
            grid_runner.main(args)
        finally:
            grid_runner.runner.post = saved
        lines = [json.loads(text) for text in (self.directory / "answers.jsonl").read_text().splitlines()]
        self.assertEqual(len(lines), 1, "the rerun resumes")
        self.assertEqual((lines[0]["route"], lines[0]["x"], lines[0]["y"]), ("r-z1000", 505, 660))
        report = grid_runner.summary(lines, [TARGET], TRUTH)
        self.assertEqual(report["by_level"], {"1": {"held": 1, "of": 1}, "2": {"held": 1, "of": 1}})
        captures = self.directory / "captures" / "captures.json"
        data = json.loads(captures.read_text())
        del data["tools"]["coordinates"]
        captures.write_text(json.dumps(data))
        with self.assertRaises(SystemExit):
            grid_runner.Job(self.directory, "z1000", grid_runner.load(self.directory))


if __name__ == "__main__":
    unittest.main()
