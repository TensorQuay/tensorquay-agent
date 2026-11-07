"""The product's set-of-marks drawing ported to Python (marks.rs `draw`), and the refs' boxes read back from a marked
screenshot: drawing what was read back on the unmarked screen gives the marked one, pixel for pixel.

Run: python3 -m unittest tests/e2e/grounding_eval/test_marks.py"""

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import marks  # noqa: E402
import pngio  # noqa: E402

GREY = (240, 240, 240)


def filled(width, height, colour=GREY):
    return pngio.Image(width, height, bytearray(bytes(colour) * width * height))


def pixel(image, x, y):
    at = (y * image.width + x) * 3
    return tuple(image.rgb[at:at + 3])


class Port(unittest.TestCase):
    """marks.rs's own TOOL-U-048 cases, restated, and the places they imply."""

    # TEST: TOOL-U-093
    def test_each_boxed_element_gets_an_outline_and_a_label_as_the_product_draws_it(self):
        image = filled(400, 300)
        given = [(1, [0, 0, 80, 30]), (2, [90, 0, 80, 30]), (12, [100, 100, 120, 40]), (13, [-500, -500, 10, 10]),
                 (14, [380, 290, 60, 40])]
        placed = marks.draw(image, given, 0)
        self.assertEqual([p[0] for p in placed], [1, 2, 12, 14], "a box off the screen gets no mark")
        self.assertEqual([p[3] for p in placed], list(marks.COLOURS), "the colours in turn")
        self.assertEqual(placed[0][2], (0, 0, 10, 14), "above the top-left, kept inside the screen")
        self.assertEqual(placed[1][2], (90, 0, 10, 14))
        self.assertEqual(placed[2][2], (100, 86, 18, 14), "two digits: (2 x 4 + 1) x 2 wide, 7 x 2 high")
        self.assertEqual(placed[3][1], (380, 290, 20, 10), "clipped to the screen")
        self.assertEqual(pixel(image, 219, 120), placed[2][3], "the outline's right edge")
        self.assertEqual(pixel(image, 217, 120), GREY, "2 px thick")
        self.assertEqual(pixel(image, 160, 120), GREY, "the box's inside untouched")
        # The label of 12: its padding, then "1" (010 110 010 010 111) from 2 px in.
        self.assertEqual(pixel(image, 100, 86), placed[2][3])
        self.assertEqual([pixel(image, 102 + 2 * column, 88) for column in range(3)],
                         [placed[2][3], marks.DIGIT, placed[2][3]])
        self.assertEqual([pixel(image, 102 + 2 * column, 96) for column in range(3)], [marks.DIGIT] * 3)

    # TEST: TOOL-U-093
    def test_crowded_labels_stay_apart_inside_the_screen_and_marks_stop_at_300(self):
        image = filled(1280, 800, (255, 255, 255))
        placed = marks.draw(image, [(i + 1, [i * 20, 200, 18, 18]) for i in range(12)], 0)
        for index, (_, _, label, _) in enumerate(placed):
            self.assertEqual(marks.within(label, 1280, 800), label)
            for other in placed[index + 1:]:
                self.assertFalse(marks.overlaps(label, other[2]), (label, other[2]))
        many = [(i + 1, [(i % 20) * 64, (i // 20) * 40, 60, 30]) for i in range(400)]
        self.assertEqual(len(marks.draw(image, many, 0)), marks.MAX_MARKS)

    # TEST: TOOL-U-093
    def test_labels_grow_so_digits_survive_scaling(self):
        full = marks.draw(filled(1280, 800), [(7, [100, 100, 50, 20])], 0)[0][2]
        small = marks.draw(filled(1280, 800), [(7, [100, 100, 50, 20])], 640)[0][2]
        self.assertEqual((full[3], small[3]), (14, 28))
        self.assertEqual(marks.unit(1280, 1280), 2, "the capture's screen at the default image side")
        self.assertEqual(marks.unit(2560, 1280), 4)

    # TEST: TOOL-U-093
    def test_a_label_goes_inside_below_or_top_right_when_the_places_before_are_taken(self):
        taken = [(100, 86, 10, 14)]
        self.assertEqual(marks.place((100, 100, 60, 30), (10, 14), (400, 300), taken), (100, 100, 10, 14), "inside")
        taken.append((100, 100, 10, 14))
        self.assertEqual(marks.place((100, 100, 60, 30), (10, 14), (400, 300), taken), (100, 130, 10, 14), "below")
        taken.append((100, 130, 10, 14))
        self.assertEqual(marks.place((100, 100, 60, 30), (10, 14), (400, 300), taken), (150, 86, 10, 14),
                         "above the top-right")
        self.assertEqual(marks.place((0, 280, 60, 30), (10, 14), (400, 300), [(0, 266, 10, 14)]), (0, 280, 10, 14),
                         "inside, the box clipped at the bottom")


def screen(width, height, given):
    """An unmarked screen with some texture, its marked twin and the text naming the refs."""
    plain = filled(width, height)
    for y in range(0, height, 7):
        marks.fill(plain, (0, y, width, 1), (220, 221, 222))
    marked = marks.copy(plain)
    marks.draw(marked, given)
    text = "Screenshot, {} elements marked\n".format(len(given)) + "".join(
        f"e{number} button \"b{number}\"\n" for number, _ in given)
    return plain, marked, text


class Recover(unittest.TestCase):
    def assert_recovered(self, given, width=640, height=400):
        plain, marked, text = screen(width, height, given)
        found, differ, count = marks.recover(plain, marked, text)
        self.assertEqual(differ, 0)
        self.assertEqual(count, len(given))
        self.assertEqual(found, [(number, list(box)) for number, box in given])

    # TEST: TOOL-U-093
    def test_a_menu_bar_whose_items_overlap_by_a_pixel(self):
        self.assert_recovered([(1, [32, 111, 47, 24]), (2, [78, 111, 49, 24]), (3, [126, 111, 55, 24]),
                               (4, [180, 111, 60, 24]), (5, [239, 111, 70, 24]), (6, [308, 111, 57, 24])])

    # TEST: TOOL-U-093
    def test_a_column_of_cells_whose_labels_hide_the_corners_above_them(self):
        given = []
        for row in range(6):
            given.append((2 * row + 1, [233, 137 + 25 * row, 35, 16]))
            given.append((2 * row + 2, [284, 135 + 25 * row, 39, 20]))
        self.assert_recovered(given)

    # TEST: TOOL-U-093
    def test_a_menu_stacked_on_a_box_reaching_the_screens_bottom(self):
        # The box's label can't go above it or inside it (the menu's labels are there), nor below: it's pushed back
        # up from the screen's bottom, as on Mousepad's open File menu over its text area.
        given = [(n, [140, 40 + 25 * (n - 1), 304, 25]) for n in range(1, 6)]
        given.append((7, [140, 65, 498, 335]))
        self.assert_recovered(given)

    # TEST: TOOL-U-093
    def test_two_digit_numbers_and_a_ref_without_a_mark(self):
        given = [(9, [20, 40, 90, 30]), (10, [130, 40, 90, 30]), (11, [240, 40, 90, 30])]
        plain, marked, text = screen(640, 400, given)
        text += 'e12 option "hidden"\n'
        found, differ, count = marks.recover(plain, marked, text)
        self.assertEqual((found, differ, count), ([(n, list(b)) for n, b in given], 0, 3))


class Refs(unittest.TestCase):
    def capture(self, folder, screens):
        (folder / "captures").mkdir(parents=True)
        records = []
        for name, given in screens.items():
            plain, marked, text = screen(320, 200, given)
            (folder / "captures" / f"{name}.png").write_bytes(pngio.encode(plain))
            (folder / "captures" / f"{name}.marked.png").write_bytes(pngio.encode(marked))
            records.append({"screen": name, "png": f"{name}.png", "marked_png": f"{name}.marked.png",
                            "width": 320, "height": 200, "text": text.replace("Screenshot,", "Screenshot 320×200,")})
        (folder / "captures" / "captures.json").write_text(json.dumps({"screens": records, "boxes": {}}))

    # TEST: TOOL-U-093
    def test_refs_json_holds_each_screens_boxes_and_the_proof_says_each_is_exact(self):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            self.capture(folder, {"a": [(1, [10, 30, 80, 20]), (2, [100, 30, 80, 20])], "b": [(1, [40, 60, 50, 50])]})
            self.assertEqual(marks.main(["refs", str(folder), str(folder / "out")]), 0)
            refs = json.loads((folder / "out" / "refs.json").read_text())
            self.assertEqual(refs, {"a": [[10, 30, 80, 20], [100, 30, 80, 20]], "b": [[40, 60, 50, 50]]})
            proof = json.loads((folder / "out" / "refs-proof.json").read_text())
            self.assertEqual((proof["screens"], proof["exact"]), (2, 2))
            self.assertEqual(proof["per_screen"]["a"]["pixels_differing"], 0)
            self.assertEqual(proof["refs_json_sha256"], marks.sha256(folder / "out" / "refs.json"))

    # TEST: TOOL-U-093
    def test_a_screen_not_reproduced_exactly_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            self.capture(folder, {"a": [(1, [10, 30, 80, 20])]})
            marked = pngio.decode((folder / "captures" / "a.marked.png").read_bytes())
            marked = marks.copy(marked)
            marks.fill(marked, (200, 150, 3, 3), marks.COLOURS[0])   # a stray patch of a mark's colour
            (folder / "captures" / "a.marked.png").write_bytes(pngio.encode(marked))
            self.assertEqual(marks.main(["refs", str(folder), str(folder / "out")]), 1)
            proof = json.loads((folder / "out" / "refs-proof.json").read_text())
            self.assertFalse(proof["per_screen"]["a"]["exact"])


class Build(unittest.TestCase):
    """A route's detector-marked screenshots: the marks file the clicks are scored from, each screen's picture and its
    tool text, merged (the product's refs, then the note's regions) or detector-only."""

    def setUp(self):
        self.folder = Path(tempfile.mkdtemp())
        Refs.capture(self, self.folder, {"a": [(1, [10, 30, 80, 20]), (2, [100, 30, 80, 20])],
                                         "c": [(1, [20, 20, 60, 16])]})
        listed = json.loads((self.folder / "captures" / "captures.json").read_text())
        for record in listed["screens"]:   # the product's words around the refs, and a ref with no mark after them
            refs = [line for line in record["text"].splitlines() if line.startswith("e")]
            record["text"] = (f"Screenshot 320×200, {len(refs)} elements marked: mark 7 is e7.\n"
                              "Snapshot 3 of the focused window (the page's structure, 3 elements). Act on an element "
                              'by its ref.\n<untrusted-content source="screen" id="x">\nWindow: W\n'
                              + "".join(line + "\n" for line in refs)
                              + 'e9 option "hidden"\n</untrusted-content id="x">')
        (self.folder / "captures" / "captures.json").write_text(json.dumps(listed))
        self.assertEqual(marks.main(["refs", str(self.folder), str(self.folder / "refs")]), 0)
        self.boxes = self.folder / "boxes.json"
        self.boxes.write_text(json.dumps({"model": "m", "kept": 0.05, "threshold": 0.35, "screens": {
            "a": {"boxes": [[12, 31, 77, 19, 0.9],           # over e1: merged drops it
                            [200.4, 100.2, 40.4, 30.6, 0.8],   # clear: rounded to [200, 100, 41, 31]
                            [201, 101, 40, 30, 0.5],           # the same thing again, lower
                            [250, 150, 20, 20, 0.2]]},         # under the threshold
            "c": {"boxes": [[150, 60, 30, 30, 0.7], [40, 120, 30, 30, 0.6]]}}}))

    def build(self, kind, route="qwen/ours-m", threshold="0.3", out="marks-out", trigger="screen"):
        return marks.main(["build", str(self.folder), str(self.folder / "refs" / "refs-proof.json"), str(self.boxes),
                           str(self.folder / out), "--route", route, "--kind", kind, "--threshold", threshold,
                           "--trigger", trigger])

    def text(self, out, screen):
        return json.loads((self.folder / out / "marks.json").read_text())["shots"][screen]["text"]

    # TEST: TOOL-U-094
    def test_merged_marks_are_the_refs_then_the_notes_regions_numbered_after_every_ref(self):
        self.assertEqual(self.build("merged"), 0)
        made = json.loads((self.folder / "marks-out" / "marks.json").read_text())
        self.assertEqual((made["route"], made["kind"], made["threshold"]), ("qwen/ours-m", "merged", 0.3))
        self.assertEqual(made["screens"]["a"], [
            {"mark": 1, "ref": "e1", "box": [10, 30, 80, 20]}, {"mark": 2, "ref": "e2", "box": [100, 30, 80, 20]},
            {"mark": 10, "ref": None, "box": [200, 100, 41, 31]}])
        self.assertEqual([m["mark"] for m in made["screens"]["c"]], [1, 10, 11], "reading order: (150, 60) first")
        plain = pngio.decode((self.folder / "captures" / "a.png").read_bytes())
        drawn = marks.copy(plain)
        marks.draw(drawn, [(1, [10, 30, 80, 20]), (2, [100, 30, 80, 20]), (10, [200, 100, 41, 31])])
        self.assertEqual(bytes(pngio.decode((self.folder / "marks-out" / "a.png").read_bytes()).rgb),
                         bytes(drawn.rgb), "one draw call, as the product would make it")
        text = self.text("marks-out", "a")
        self.assertIn("mark 7 is e7. Mark 10 is a region found in the picture, with no name: look at the image to "
                      "see what it is.\n", text)
        self.assertIn('e9 option "hidden"\ne10 region (from the picture)\n</untrusted-content', text)
        self.assertIn("Marks 10 to 11 are regions", self.text("marks-out", "c"))

    # TEST: TOOL-U-094
    def test_detector_only_marks_every_box_at_the_threshold_from_e1_with_no_structure(self):
        self.assertEqual(self.build("detector-only", route="qwen/ours-d"), 0)
        made = json.loads((self.folder / "marks-out" / "marks.json").read_text())
        self.assertEqual(made["screens"]["a"], [
            {"mark": 1, "ref": None, "box": [12, 31, 77, 19]}, {"mark": 2, "ref": None, "box": [200, 100, 41, 31]},
            {"mark": 3, "ref": None, "box": [201, 101, 40, 30]}])
        text = self.text("marks-out", "a")
        self.assertTrue(text.startswith("Screenshot 320×200, no element marked from the window's structure. Marks 1 to "
                                        "3 are regions found in the picture"), text)
        self.assertNotIn("e9 option", text)
        self.assertIn("(the page's structure, 0 elements)", text)
        self.assertIn("Window: W\ne1 region (from the picture)\ne2 region", text)
        plain = pngio.decode((self.folder / "captures" / "a.png").read_bytes())
        drawn = marks.copy(plain)
        marks.draw(drawn, [(1, [12, 31, 77, 19]), (2, [200, 100, 41, 31]), (3, [201, 101, 40, 30])])
        self.assertEqual(bytes(pngio.decode((self.folder / "marks-out" / "a.png").read_bytes()).rgb),
                         bytes(drawn.rgb))

    # TEST: TOOL-U-094
    def test_by_the_notes_triggers_only_a_screen_with_under_5_boxed_refs_gets_regions(self):
        listed = json.loads((self.folder / "captures" / "captures.json").read_text())
        many = [(n, [10 + 60 * (n - 1), 30, 50, 20]) for n in range(1, 6)]   # 5 boxed refs: "few" doesn't fire
        Refs.capture(self, self.folder / "five", {"a": many})
        five = json.loads((self.folder / "five" / "captures" / "captures.json").read_text())["screens"][0]
        (self.folder / "captures" / "a.png").write_bytes((self.folder / "five" / "captures" / "a.png").read_bytes())
        (self.folder / "captures" / "a.marked.png").write_bytes(
            (self.folder / "five" / "captures" / "a.marked.png").read_bytes())
        listed["screens"][0] = five
        (self.folder / "captures" / "captures.json").write_text(json.dumps(listed))
        self.assertEqual(marks.main(["refs", str(self.folder), str(self.folder / "refs")]), 0)
        self.assertEqual(self.build("merged", trigger="note"), 0)
        made = json.loads((self.folder / "marks-out" / "marks.json").read_text())
        self.assertEqual([m["ref"] for m in made["screens"]["a"]], ["e1", "e2", "e3", "e4", "e5"], "no regions")
        self.assertEqual((self.folder / "marks-out" / "a.png").read_bytes() != b"", True)
        self.assertEqual(bytes(pngio.decode((self.folder / "marks-out" / "a.png").read_bytes()).rgb),
                         bytes(pngio.decode((self.folder / "captures" / "a.marked.png").read_bytes()).rgb),
                         "today's screenshot, pixel for pixel")
        self.assertEqual(made["shots"]["a"]["text"], five["text"], "and today's words")
        self.assertEqual([m["mark"] for m in made["screens"]["c"]], [1, 10, 11], "1 boxed ref: 'few' fires")
        self.assertEqual(made["trigger"], "note")
        with self.assertRaises(SystemExit):
            self.build("merged", trigger="window")

    # TEST: TOOL-U-094
    def test_a_threshold_under_what_the_detector_kept_or_a_proof_not_exact_is_refused(self):
        with self.assertRaises(SystemExit) as refused:
            self.build("merged", threshold="0.01")
        self.assertIn("below the 0.05", str(refused.exception))
        proof = json.loads((self.folder / "refs" / "refs-proof.json").read_text())
        proof["per_screen"]["a"]["exact"] = False
        (self.folder / "refs" / "refs-proof.json").write_text(json.dumps(proof))
        with self.assertRaises(SystemExit) as refused:
            self.build("merged")
        self.assertIn("not exact", str(refused.exception))


if __name__ == "__main__":
    unittest.main()
