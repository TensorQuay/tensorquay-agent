"""The grid runner without a model: the conversation it builds turn by turn (the level-1 grid, then each zoom the model
asks for), the point it reads from the final cell, a miss with no further call when a pick's window can't hold the
target, the refusals (coordinates written, a bad label, no tool call), its answer line, a run that resumes, and four
threads drawing the same grid image at once.

Run: python3 -m unittest tests/e2e/grounding_eval/test_grid_runner.py"""

import json
import os
import sys
import tempfile
import threading
import unittest
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import grid  # noqa: E402
import grid_runner  # noqa: E402
import pngio  # noqa: E402

TARGET = {"id": "form-01-01", "kind": "form", "page": "form-01", "instruction": "Click the Email field."}
# The target: 40 x 20 at (485, 650): its middle (505, 660) is in level-1 cell D5 (480-640, 640-800).
TRUTH = {"form-01-01": {"ref": "e1", "box": [485, 650, 40, 20]}}
MARK_SPEC = {"name": "computer", "description": "Operate the desktop.",
             "parameters": {"type": "object", "additionalProperties": False, "required": ["action", "ref"],
                            "properties": {"action": {"type": "string", "enum": ["screenshot", "click"]},
                                           "ref": {"type": ["string", "null"]}}}}


def call(arguments, finish="tool_calls"):
    return {"choices": [{"finish_reason": finish, "message": {"tool_calls": [
        {"function": {"name": "computer", "arguments": json.dumps(arguments)}}]}}],
        "usage": {"prompt_tokens": 1500, "completion_tokens": 100}}


class Setup(unittest.TestCase):
    def setUp(self):
        self.directory = Path(tempfile.mkdtemp())
        captures = self.directory / "captures"
        captures.mkdir()
        screen = pngio.Image(1280, 800, bytearray([250, 250, 250] * 1280 * 800))
        (captures / "form-01.png").write_bytes(pngio.encode(screen))
        (captures / "captures.json").write_text(json.dumps({
            "screens": [{"screen": "form-01", "png": "form-01.png", "width": 1280, "height": 800}],
            "boxes": TRUTH, "tools": {"mark": MARK_SPEC}}))
        (self.directory / "targets.json").write_text(json.dumps({"screen": [1280, 800], "targets": [TARGET]}))
        self.sent = []

    def replies(self, *answers):
        queue = list(answers)

        def post(base_url, model, body, key=None):
            self.sent.append(body)
            return queue.pop(0)
        return post

    def run_one(self, variant, *answers):
        job = grid_runner.Job(self.directory, variant, grid_runner.load(self.directory))
        return job.answer(TARGET, self.replies(*answers), "http://m", "m", None, 16384, {})


class Turns(Setup):
    # TEST: TOOL-U-074
    def test_g12_level_1_then_a_zoom_then_a_click_on_the_fine_cell(self):
        # D5 is on the bottom row, so its G12 window (cell plus 10 px) is shifted up inside the screen: (470, 620, 180,
        # 180). Fine cell C3 is x 494-506, y 644-656: centre (500, 650), inside the target (x 485-525, y 650-670).
        line = self.run_one("g12", call({"action": "grid", "cell": "D5"}), call({"action": "click", "cell": "C3"}))
        self.assertEqual((line["x"], line["y"], line["image"]), (500, 650, [1280, 800]))
        self.assertEqual(line["cells"], ["D5", "C3"])
        self.assertEqual([level["holds_target"] for level in line["levels"]], [True, True])
        self.assertEqual((line["way"], line["variant"], line["calls"]), ("coordinates", "g12", 2))
        first, second = self.sent
        self.assertEqual(first["messages"][2]["tool_calls"][0]["function"]["arguments"], json.dumps({"action": "grid"}))
        self.assertTrue(first["messages"][3]["content"].startswith("Grid 1280×800: 8 columns"))
        self.assertEqual(first["messages"][4]["content"][1]["type"], "image_url")
        self.assertTrue(second["messages"][-2]["content"].startswith("Cell D5, 3.6 times closer"))
        self.assertEqual(sum(1 for m in second["messages"] if isinstance(m["content"], list)), 2, "two images")
        self.assertEqual(first["tools"][0]["function"]["parameters"]["properties"]["cell"]["type"], ["string", "null"])
        self.assertEqual(line["usage"]["completion_tokens"], 200, "summed over the calls")

    # TEST: TOOL-U-074
    def test_g20_may_zoom_a_third_time_and_never_sends_more_than_three_images(self):
        # D5's G20 window, shifted up: (460, 600, 200, 200). Cell C3 is x 500-520, y 640-660; its level 3 window is
        # (490, 630, 40, 40), whose B3 is x 500-510, y 650-660: centre (505, 655).
        line = self.run_one("g20", call({"action": "grid", "cell": "D5"}), call({"action": "grid", "cell": "C3"}),
                            call({"action": "click", "cell": "B3"}))
        self.assertEqual(line["cells"], ["D5", "C3", "B3"])
        self.assertEqual((line["x"], line["y"]), (505, 655))
        self.assertTrue(line["third_zoom"])
        self.assertEqual(sum(1 for m in self.sent[-1]["messages"] if isinstance(m["content"], list)), 3)

    # TEST: TOOL-U-074
    def test_a_pick_whose_window_cant_hold_the_target_ends_the_target_with_no_further_call(self):
        line = self.run_one("g12", call({"action": "grid", "cell": "A1"}))
        self.assertEqual(line["error"], "picked a cell whose window can't hold the target")
        self.assertEqual(line["levels"], [{"level": 1, "cell": "A1", "holds_target": False}])
        self.assertEqual(len(self.sent), 1, "no zoom drawn, no call made")

    # TEST: TOOL-U-074
    def test_a_click_on_level_1_is_scored_at_its_centre(self):
        line = self.run_one("g12", call({"action": "click", "cell": "D5"}))
        self.assertEqual((line["x"], line["y"], line["cells"]), (560, 720, ["D5"]))

    # TEST: TOOL-U-074
    def test_the_refusals_are_misses_that_say_why(self):
        cases = [
            (call({"action": "click", "x": 500, "y": 660}), "wrote coordinates"),
            (call({"action": "click", "cell": "Z9"}), "no such cell"),
            (call({"action": "click", "ref": "e3"}), "no cell"),
            ({"choices": [{"finish_reason": "length", "message": {}}], "usage": {}}, "no tool call"),
        ]
        for reply, error in cases:
            self.sent.clear()
            self.assertEqual(self.run_one("g12", reply)["error"], error, error)
        # A zoom past the last level is refused as the tool would refuse it.
        line = self.run_one("g12", call({"action": "grid", "cell": "D5"}), call({"action": "grid", "cell": "C3"}))
        self.assertEqual(line["error"], "the cells can't get smaller")


class Run(Setup):
    # TEST: TOOL-U-074
    def test_a_run_writes_one_line_per_target_and_resumes(self):
        saved = grid_runner.runner.post
        grid_runner.runner.post = self.replies(call({"action": "grid", "cell": "D5"}),
                                               call({"action": "click", "cell": "C3"}))
        try:
            args = [str(self.directory), "--variant", "g12", "--route", "r-g12", "--base-url", "http://m",
                    "--model", "m"]
            grid_runner.main(args)
            grid_runner.main(args)
        finally:
            grid_runner.runner.post = saved
        lines = [json.loads(text) for text in (self.directory / "answers.jsonl").read_text().splitlines()]
        self.assertEqual(len(lines), 1, "the rerun resumes")
        self.assertEqual((lines[0]["route"], lines[0]["x"]), ("r-g12", 500))
        report = grid_runner.summary(lines, [TARGET], TRUTH)
        self.assertEqual(report["by_level"], {"1": {"held": 1, "of": 1}, "2": {"held": 1, "of": 1}})
        with self.assertRaises(SystemExit):
            grid_runner.main([str(self.directory), "--variant", "g20", "--route", "r-g12", "--base-url", "http://m",
                              "--model", "m"])


class Cache(Setup):
    # TEST: TOOL-U-074
    def test_four_threads_drawing_the_same_image_at_once_all_get_it(self):
        # The Saturday hold (24 Oct 2026): two of the run's threads drew form-01's level 1 together and wrote one
        # temporary file; the first moved it into place and the second's move found nothing (FileNotFoundError). The
        # barrier holds every thread after its write and before its move, so all four have written before any moves.
        job = grid_runner.Job(self.directory, "g12", grid_runner.load(self.directory))
        barrier, replace = threading.Barrier(4, timeout=10), os.replace

        def replace_together(source, target):
            barrier.wait()
            return replace(source, target)
        os.replace = replace_together
        self.addCleanup(setattr, os, "replace", replace)
        with ThreadPoolExecutor(4) as pool:
            got = list(pool.map(lambda _: job.image("form-01", grid.level1(1280, 800)), range(4)))
        cache = self.directory / "grid-cache" / "g12" / "form-01"
        self.assertEqual(sorted(p.name for p in cache.iterdir()), ["top.png"], "no temporary file left")
        self.assertEqual(got, [(cache / "top.png").read_bytes()] * 4)


if __name__ == "__main__":
    unittest.main()
