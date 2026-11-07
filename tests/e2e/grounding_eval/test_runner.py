"""The grounding evaluation's runner without a model: each way's conversation (what the model is shown), the tool's
schema as the capture recorded the product offering it, the request as the route sends it, the answer lines read from
replies, and a run of several calls at once that resumes.

Run: python3 -m unittest tests/e2e/grounding_eval/test_runner.py"""

import base64
import json
import sys
import tempfile
import threading
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import runner  # noqa: E402

TARGET = {"id": "form-01-01", "kind": "form", "page": "form-01", "instruction": "Click the Email field."}
SCREEN = {"screen": "form-01", "png": "form-01.png", "marked_png": "form-01.marked.png", "width": 1280,
          "height": 800, "text": "Screenshot 1280×800, 10 elements marked...\n<snapshot>"}


def reply(arguments, finish="tool_calls"):
    return {"choices": [{"finish_reason": finish,
                         "message": {"tool_calls": [{"function": {"name": "computer",
                                                                  "arguments": json.dumps(arguments)}}]}}],
            "usage": {"prompt_tokens": 900, "completion_tokens": 120}}


def offered(*fields):
    return {"name": "computer", "description": "Operate the desktop.",
            "parameters": {"type": "object", "properties": {field: {} for field in fields}}}


# As the capture writes them: the product's own schema for each way (the tool's spec on each kind of route).
TOOLS = {"ref": offered("action", "ref"), "mark": offered("action", "ref"),
         "coordinates": offered("action", "ref", "x", "y", "to")}


class Conversation(unittest.TestCase):
    def setUp(self):
        self.directory = Path(tempfile.mkdtemp())
        (self.directory / "captures").mkdir()
        (self.directory / "captures" / "form-01.png").write_bytes(b"plain")
        (self.directory / "captures" / "form-01.marked.png").write_bytes(b"marked")

    def test_by_ref_shows_the_text_alone(self):
        shown = runner.messages(TARGET, SCREEN, "ref", self.directory)
        self.assertEqual(shown[-1]["role"], "tool")
        self.assertEqual(shown[-1]["content"], SCREEN["text"])
        self.assertEqual(json.loads(shown[2]["tool_calls"][0]["function"]["arguments"]), {"action": "observe"})

    def test_by_mark_shows_the_marked_image_and_by_coordinates_the_plain_one_without_its_refs(self):
        marked = runner.messages(TARGET, SCREEN, "mark", self.directory)
        self.assertTrue(marked[-1]["content"][1]["image_url"]["url"].endswith("bWFya2Vk"))  # "marked"
        plain = runner.messages(TARGET, SCREEN, "coordinates", self.directory)
        self.assertTrue(plain[-1]["content"][1]["image_url"]["url"].endswith("cGxhaW4="))  # "plain"
        # The product's own words for a screen where nothing has a mark, on a route with pixel_grounding: no refs to
        # lean on, and told so, as a canvas screen tells it (computer.rs, the screenshot's text).
        self.assertEqual(plain[3]["content"],
                         "Screenshot 1280×800, no element has a mark: reach what you see with keys, or by x and y.")

    def test_no_request_carries_more_images_than_the_server_takes(self):
        for way in runner.WAYS:
            shown = runner.messages(TARGET, SCREEN, way, self.directory)
            images = sum(1 for message in shown if isinstance(message["content"], list)
                         for part in message["content"] if part.get("type") == "image_url")
            self.assertLessEqual(images, runner.IMAGES_PER_REQUEST, way)
            self.assertLessEqual(runner.IMAGES_PER_REQUEST, 3, "TQA_LOCAL_MODEL_IMAGES=3")

    def test_each_way_sends_the_products_own_schema_from_the_capture(self):
        for way in runner.WAYS:
            sent = runner.tool(TOOLS, way)
            self.assertEqual(sent, {"type": "function", "function": TOOLS[way]}, way)
        self.assertNotIn("x", runner.tool(TOOLS, "mark")["function"]["parameters"]["properties"])
        with self.assertRaises(SystemExit):
            runner.tool({}, "ref")

    def test_the_request_is_the_routes_own(self):
        body = runner.request(TARGET, SCREEN, "mark", self.directory, TOOLS, 16384,
                              {"chat_template_kwargs": {"enable_thinking": True}})
        # The product sends no tool_choice on OpenAI Chat (auto), and the route's own output bound and extra body.
        self.assertNotIn("tool_choice", body)
        self.assertNotIn("temperature", body, "the server's default, as the route has no temperature")
        self.assertEqual(body["max_tokens"], 16384)
        self.assertEqual(body["chat_template_kwargs"], {"enable_thinking": True})
        self.assertEqual(body["tools"], [runner.tool(TOOLS, "mark")])


class Answers(unittest.TestCase):
    def test_why_it_ended_and_its_tokens_are_kept(self):
        line = runner.answer(reply({"action": "click", "ref": "e3"}), TARGET, "r", "ref", SCREEN)
        self.assertEqual((line["finish_reason"], line["usage"]["completion_tokens"]), ("tool_calls", 120))
        # A thinking reply cut at its bound has no call: a miss, and the line says why.
        cut = runner.answer({"choices": [{"finish_reason": "length", "message": {"content": None}}],
                             "usage": {"completion_tokens": 16384}}, TARGET, "r", "ref", SCREEN)
        self.assertEqual((cut["error"], cut["finish_reason"]), ("no tool call", "length"))

    def test_a_miss_records_what_the_call_did(self):
        # The first run's 79 coordinates answers without a point couldn't say what they were (12 Oct 2026).
        looked = runner.answer(reply({"action": "screenshot", "ref": None, "x": None, "y": None}), TARGET, "r",
                               "coordinates", SCREEN)
        self.assertEqual((looked["error"], looked["action"]), ("no point", "screenshot"))
        self.assertEqual(looked["arguments"], {"action": "screenshot", "ref": None, "x": None, "y": None})
        named = runner.answer(reply({"action": "click", "ref": "e3"}), TARGET, "r", "coordinates", SCREEN)
        self.assertEqual((named["error"], named["action"], named["ref"]), ("no point", "click", "e3"))
        # A hit keeps its line short: the action, not the arguments.
        hit = runner.answer(reply({"action": "click", "x": 5, "y": 6}), TARGET, "r", "coordinates", SCREEN)
        self.assertEqual(hit["action"], "click")
        self.assertNotIn("arguments", hit)
        unread = runner.answer({"choices": [{"message": {"tool_calls": [{"function": {"arguments": "{oops"}}]}}]},
                               TARGET, "r", "ref", SCREEN)
        self.assertEqual((unread["error"], unread["arguments"]), ("unparsable arguments", "{oops"))

    def test_a_point_a_ref_and_the_misses(self):
        point = runner.answer(reply({"action": "click", "x": 12, "y": 34}), TARGET, "r", "coordinates", SCREEN)
        self.assertEqual((point["x"], point["y"], point["image"]), (12, 34, [1280, 800]))
        mark = runner.answer(reply({"action": "click", "ref": 7}), TARGET, "r", "mark", SCREEN)
        self.assertEqual(mark["ref"], "7")
        self.assertEqual(runner.answer({"choices": [{"message": {}}]}, TARGET, "r", "ref", SCREEN)["error"],
                         "no tool call")
        self.assertEqual(runner.answer(reply({"action": "click", "ref": "e1"}), TARGET, "r", "coordinates",
                                       SCREEN)["error"], "no point")


# The product's coordinate fields as the capture records them on a route with pixel_grounding (args.rs, add_points).
PIXEL_X = {"type": ["integer", "null"], "minimum": 0, "maximum": 4096,
           "description": "For click, double_click, scroll and drag: the point's x in the latest screenshot's pixels, "
                          "instead of a ref; use a ref or a mark whenever the element has one"}
POINT = {"type": "object", "additionalProperties": False, "required": ["x", "y"],
         "properties": {"x": {"type": "integer", "minimum": 0, "maximum": 4096},
                        "y": {"type": "integer", "minimum": 0, "maximum": 4096}}}
RECORDED = {"name": "computer", "description": "Operate the desktop.",
            "parameters": {"type": "object", "properties": {
                "action": {}, "ref": {}, "x": PIXEL_X, "y": {**PIXEL_X, "description": PIXEL_X["description"].replace(
                    "x in", "y in")},
                "to": {"anyOf": [POINT, {"type": "null"}],
                       "description": "For drag: where to, in the latest screenshot's pixels"}}}}


class Relative1000(unittest.TestCase):
    """The exploratory 0-1000 run (c17): --coordinate-space 1000 asks the coordinates way for x and y from
    0 to 1000 across the screenshot, Qwen3-VL's family's grounding convention, without any product change."""

    def setUp(self):
        self.directory = Path(tempfile.mkdtemp())
        (self.directory / "captures").mkdir()
        (self.directory / "captures" / "form-01.png").write_bytes(b"plain")

    # TEST: TOOL-U-069
    def test_the_recorded_spec_says_0_to_1000_and_bounds_it_and_only_for_coordinates(self):
        tools = {"ref": offered("action", "ref"), "mark": offered("action", "ref"), "coordinates": RECORDED}
        sent = runner.tool(tools, "coordinates", "1000")["function"]["parameters"]["properties"]
        for axis in ("x", "y"):
            self.assertEqual(sent[axis]["maximum"], 1000, axis)
            self.assertEqual(sent[axis]["minimum"], 0, axis)
            self.assertEqual(sent[axis]["type"], ["integer", "null"], axis)
            self.assertIn(runner.RELATIVE_WORDS, sent[axis]["description"])
            self.assertNotIn("pixels", sent[axis]["description"])
        self.assertTrue(sent["y"]["description"].startswith("For click, double_click, scroll and drag: the point's y"))
        to = sent["to"]
        self.assertNotIn("pixels", to["description"])
        self.assertIn(runner.RELATIVE_WORDS, to["description"])
        self.assertEqual({axis: to["anyOf"][0]["properties"][axis]["maximum"] for axis in ("x", "y")},
                         {"x": 1000, "y": 1000})
        # The capture's record itself is left as it was, and pixels and the other ways send it unchanged.
        self.assertEqual(RECORDED["parameters"]["properties"]["x"], PIXEL_X)
        self.assertEqual(runner.tool(tools, "coordinates", "pixels"), {"type": "function", "function": RECORDED})
        self.assertEqual(runner.tool(tools, "mark", "1000"), {"type": "function", "function": tools["mark"]})

    # TEST: TOOL-U-069
    def test_the_result_text_says_0_to_1000(self):
        shown = runner.messages(TARGET, SCREEN, "coordinates", self.directory, "1000")
        self.assertEqual(shown[3]["content"],
                         "Screenshot 1280×800, no element has a mark: reach what you see with keys, or by x and y "
                         "from 0 to 1000 across the screenshot, (0, 0) top-left and (1000, 1000) bottom-right.")
        pixels = runner.messages(TARGET, SCREEN, "coordinates", self.directory)
        self.assertEqual(pixels[3]["content"],
                         "Screenshot 1280×800, no element has a mark: reach what you see with keys, or by x and y.")

    # TEST: TOOL-U-069
    def test_an_answer_records_the_1000_image_and_its_space(self):
        point = runner.answer(reply({"action": "click", "x": 1000, "y": 0}), TARGET, "r", "coordinates", SCREEN,
                              "1000")
        self.assertEqual((point["x"], point["y"], point["image"], point["coordinate_space"]),
                         (1000, 0, [1000, 1000], "1000"))
        pixels = runner.answer(reply({"action": "click", "x": 12, "y": 34}), TARGET, "r", "coordinates", SCREEN)
        self.assertEqual(pixels["image"], [1280, 800])
        self.assertNotIn("coordinate_space", pixels, "a pixels line is as before")
        marked = runner.answer(reply({"action": "click", "ref": 7}), TARGET, "r", "mark", SCREEN, "1000")
        self.assertNotIn("coordinate_space", marked, "only the coordinates way has a space")
        missed = runner.answer(reply({"action": "click", "ref": "e3"}), TARGET, "r", "coordinates", SCREEN, "1000")
        self.assertEqual((missed["error"], missed["coordinate_space"]), ("no point", "1000"), "a miss keeps its space")


class Run(unittest.TestCase):
    """main() with the model server replaced: several calls at once, each answer once, and a rerun resumes."""

    def setUp(self):
        self.directory = Path(tempfile.mkdtemp())
        (self.directory / "captures").mkdir()
        for name in ("form-01.png", "form-01.marked.png"):
            (self.directory / "captures" / name).write_bytes(b"x")
        targets = [{**TARGET, "id": f"form-01-{n:02d}"} for n in range(1, 9)]
        (self.directory / "targets.json").write_text(json.dumps({"targets": targets}))
        (self.directory / "captures" / "captures.json").write_text(
            json.dumps({"screens": [SCREEN], "boxes": {}, "tools": TOOLS}))
        self.calls, self.lock, self.active, self.most = [], threading.Lock(), 0, 0
        self.saved = runner.post

        def post(base_url, model, body, key=None):
            with self.lock:
                self.active += 1
                self.most = max(self.most, self.active)
                self.calls.append(body)
            threading.Event().wait(0.02)
            with self.lock:
                self.active -= 1
            return reply({"action": "click", "ref": "e1"})

        runner.post = post

    def tearDown(self):
        runner.post = self.saved

    def test_four_at_a_time_each_answer_once_and_a_rerun_resumes(self):
        args = [str(self.directory), "--route", "r", "--base-url", "http://model", "--model", "m",
                "--concurrency", "4", "--max-tokens", "8192"]
        runner.main(args)
        lines = [json.loads(line) for line in (self.directory / "answers.jsonl").read_text().splitlines()]
        self.assertEqual(len(lines), 24, "8 targets x 3 ways")
        self.assertEqual(len({(line["way"], line["target"]) for line in lines}), 24)
        self.assertGreater(self.most, 1, "calls overlap")
        self.assertLessEqual(self.most, 4)
        self.assertTrue(all(body["max_tokens"] == 8192 for body in self.calls))
        runner.main(args)
        self.assertEqual(len((self.directory / "answers.jsonl").read_text().splitlines()), 24, "nothing again")

    # TEST: TOOL-U-072
    def test_a_canvas_target_is_asked_only_by_coordinates(self):
        # A canvas target has no element, so no ref or mark can name it: it goes on the coordinates way only.
        canvas = {**TARGET, "id": "canvas-01-01", "kind": "canvas", "anchor": {"role": "heading", "name": "T"},
                  "box_in_anchor": [36, 52, 36, 36]}
        listed = json.loads((self.directory / "targets.json").read_text())["targets"]
        (self.directory / "targets.json").write_text(json.dumps({"targets": listed + [canvas]}))
        runner.main([str(self.directory), "--route", "r", "--base-url", "http://model", "--model", "m"])
        lines = [json.loads(line) for line in (self.directory / "answers.jsonl").read_text().splitlines()]
        self.assertEqual(len(lines), 8 * 3 + 1)
        self.assertEqual([line["way"] for line in lines if line["target"] == "canvas-01-01"], ["coordinates"])

    # TEST: TOOL-U-069
    def test_a_1000_run_sends_the_1000_words_and_never_shares_a_route_with_a_pixels_run(self):
        (self.directory / "captures" / "captures.json").write_text(
            json.dumps({"screens": [SCREEN], "boxes": {}, "tools": {**TOOLS, "coordinates": RECORDED}}))
        base = [str(self.directory), "--base-url", "http://model", "--model", "m", "--ways", "coordinates"]
        runner.main(base + ["--route", "r-1000", "--coordinate-space", "1000", "--limit", "2"])
        sent = self.calls[-1]["tools"][0]["function"]["parameters"]["properties"]["x"]
        self.assertEqual(sent["maximum"], 1000)
        self.assertIn("from 0 to 1000", self.calls[-1]["messages"][3]["content"])
        lines = [json.loads(line) for line in (self.directory / "answers.jsonl").read_text().splitlines()]
        self.assertEqual({line["coordinate_space"] for line in lines}, {"1000"})
        # A pixels run on that route would mix two spaces under one name (the scorer keys by route): refused.
        with self.assertRaises(SystemExit) as refused:
            runner.main(base + ["--route", "r-1000"])
        self.assertIn("another --route", str(refused.exception))
        runner.main(base + ["--route", "r"])
        self.assertEqual(len((self.directory / "answers.jsonl").read_text().splitlines()), 10, "2 + 8 pixels")
        with self.assertRaises(SystemExit):
            runner.main(base + ["--route", "r", "--coordinate-space", "1000"])


class Marks(unittest.TestCase):
    """--marks: a route's detector-marked screenshots (marks.py build) in place of the capture's own."""

    def setUp(self):
        self.directory = Path(tempfile.mkdtemp())
        (self.directory / "captures").mkdir()
        for name in ("form-01.png", "form-01.marked.png"):
            (self.directory / "captures" / name).write_bytes(b"capture")
        canvas = {**TARGET, "id": "canvas-01-01", "kind": "canvas", "anchor": {"role": "heading", "name": "T"},
                  "box_in_anchor": [36, 52, 36, 36]}
        (self.directory / "targets.json").write_text(json.dumps({"targets": [TARGET, canvas]}))
        (self.directory / "captures" / "captures.json").write_text(
            json.dumps({"screens": [SCREEN], "boxes": {}, "tools": TOOLS}))
        (self.directory / "marks-r").mkdir()
        (self.directory / "marks-r" / "form-01.png").write_bytes(b"regions")
        (self.directory / "marks-r" / "marks.json").write_text(json.dumps({
            "route": "r", "kind": "merged", "description": "Regions too.",
            "screens": {"form-01": [{"mark": 13, "ref": None, "box": [1, 2, 3, 4]}]},
            "shots": {"form-01": {"png": "form-01.png", "text": "Screenshot with regions"}}}))
        self.calls, self.saved = [], runner.post

        def post(base_url, model, body, key=None):
            self.calls.append(body)
            return reply({"action": "click", "ref": "e13"})
        runner.post = post

    def tearDown(self):
        runner.post = self.saved

    def run_marks(self, *more):
        runner.main([str(self.directory), "--base-url", "http://model", "--model", "m", "--marks", "marks-r",
                     "--ways", "mark", *more])

    # TEST: TOOL-U-094
    def test_every_target_canvas_too_is_asked_by_mark_on_the_routes_own_picture_and_words(self):
        self.run_marks("--route", "r")
        lines = [json.loads(line) for line in (self.directory / "answers.jsonl").read_text().splitlines()]
        self.assertEqual(sorted((line["target"], line["way"], line["ref"]) for line in lines),
                         [("canvas-01-01", "mark", "e13"), ("form-01-01", "mark", "e13")])
        body = self.calls[0]
        self.assertEqual(body["messages"][3]["content"], "Screenshot with regions")
        image = body["messages"][4]["content"][1]["image_url"]["url"]
        self.assertEqual(image, "data:image/png;base64," + base64.b64encode(b"regions").decode())
        self.assertEqual(body["tools"][0]["function"]["description"], "Operate the desktop. Regions too.")

    # TEST: TOOL-U-094
    def test_marks_for_another_route_or_another_way_are_refused(self):
        with self.assertRaises(SystemExit) as refused:
            self.run_marks("--route", "other")
        self.assertIn("for route 'r'", str(refused.exception))
        with self.assertRaises(SystemExit) as refused:
            runner.main([str(self.directory), "--base-url", "http://model", "--model", "m", "--marks", "marks-r",
                         "--route", "r"])
        self.assertIn("--ways mark", str(refused.exception))
        self.assertEqual(self.calls, [])


if __name__ == "__main__":
    unittest.main()
