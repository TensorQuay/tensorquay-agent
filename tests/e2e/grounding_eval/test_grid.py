"""Grid-then-zoom, without a model (c17, exploratory): the two variants' geometry (G12: 160 px, then 15 x 15 cells of
12 px; G20: 160 px, then 10 x 10 of 20 px, then 4 x 4 of 10 px), labels without I or O, windows shifted inside the
screen at its edges, every target of at least 13 px reachable from the cell holding its middle, the drawn image (lines,
a label inside each cell), the PNG round trip, and the words and the tool spec the runner sends.

Run: python3 -m unittest tests/e2e/grounding_eval/test_grid.py"""

import random
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import grid  # noqa: E402
import pngio  # noqa: E402

SCREEN = (1280, 800)


class Geometry(unittest.TestCase):
    # TEST: TOOL-U-074
    def test_level_1_is_8_by_5_cells_of_160_px_labelled_a1_to_h5(self):
        view = grid.level1(*SCREEN)
        self.assertEqual((view.window, view.cell, view.cols, view.rows), ((0, 0, 1280, 800), 160, 8, 5))
        self.assertEqual(grid.labels(view)[:3], ["A1", "B1", "C1"])
        self.assertEqual(grid.labels(view)[-1], "H5")
        self.assertEqual(grid.cell_rect(view, "C4"), (320, 480, 160, 160))
        self.assertEqual(grid.centre(view, "A1"), (80, 80))
        self.assertIsNone(grid.parse(view, "I1"), "no I")
        self.assertIsNone(grid.parse(view, "A6"), "no row 6")
        self.assertEqual(grid.parse(view, " c4 "), (2, 3), "case and spaces forgiven")

    # TEST: TOOL-U-074
    def test_another_screen_cuts_the_last_row_short(self):
        view = grid.level1(1366, 768)
        self.assertEqual((view.cols, view.rows), (9, 5))
        self.assertEqual(grid.cell_rect(view, "J5"), (1280, 640, 86, 128), "the last column and row cut short")

    # TEST: TOOL-U-074
    def test_the_columns_skip_i_and_o(self):
        self.assertEqual(grid.COLUMNS[:15], "ABCDEFGHJKLMNPQ")
        g12 = grid.zoom(grid.level1(*SCREEN), "C4", "g12")
        self.assertEqual((g12.cols, g12.rows, g12.cell), (15, 15, 12))
        self.assertEqual(grid.labels(g12)[14], "Q1")
        g20 = grid.zoom(grid.level1(*SCREEN), "C4", "g20")
        self.assertEqual((g20.cols, g20.rows, g20.cell), (10, 10, 20))
        self.assertEqual(grid.labels(g20)[9], "K1")

    # TEST: TOOL-U-074
    def test_each_zoom_is_the_cell_plus_its_margin_shifted_inside_the_screen(self):
        top = grid.level1(*SCREEN)
        self.assertEqual(grid.zoom(top, "C4", "g12").window, (310, 470, 180, 180))
        self.assertEqual(grid.zoom(top, "C4", "g20").window, (300, 460, 200, 200))
        self.assertEqual(grid.zoom(top, "A1", "g20").window, (0, 0, 200, 200), "shifted in at the top left")
        self.assertEqual(grid.zoom(top, "H5", "g20").window, (1080, 600, 200, 200), "and at the bottom right")
        self.assertEqual(grid.zoom(top, "H1", "g12").window, (1100, 0, 180, 180))
        level2 = grid.zoom(top, "C4", "g20")
        level3 = grid.zoom(level2, "F7", "g20")
        self.assertEqual((level3.window, level3.cell, level3.cols), ((390, 570, 40, 40), 10, 4))
        self.assertEqual(level3.path, ["C4", "F7"])
        self.assertIsNone(grid.zoom(level3, "A1", "g20"), "no finer level")
        self.assertIsNone(grid.zoom(grid.zoom(top, "C4", "g12"), "A1", "g12"), "G12 has two levels")

    # TEST: TOOL-U-074
    def test_a_fine_cells_centre_on_the_screen(self):
        g12 = grid.zoom(grid.level1(*SCREEN), "C4", "g12")
        self.assertEqual(grid.centre(g12, "A1"), (316, 476))
        self.assertEqual(grid.centre(g12, "Q15"), (310 + 14 * 12 + 6, 470 + 14 * 12 + 6))
        level3 = grid.zoom(grid.zoom(grid.level1(*SCREEN), "C4", "g20"), "F7", "g20")
        self.assertEqual(grid.centre(level3, "D4"), (390 + 35, 570 + 35))

    # TEST: TOOL-U-074
    def test_every_target_of_13_px_or_more_is_reachable_from_the_cell_holding_its_middle(self):
        # The design's condition: a final cell no larger than the smallest side, and a margin for a split target.
        rng = random.Random(20261022)
        boxes = [[292, 216, 14, 13], [150, 150, 13, 13], [159, 799 - 13, 13, 13], [0, 0, 13, 13], [1267, 787, 13, 13]]
        boxes += [[rng.randrange(0, 1280 - w), rng.randrange(0, 800 - h), w, h]
                  for w, h in ((rng.randrange(13, 60), rng.randrange(13, 40)) for _ in range(3000))]
        for variant in ("g12", "g20"):
            missed = [box for box in boxes if not grid.reachable(box, variant, *SCREEN)]
            self.assertEqual(missed, [], variant)
        # G20 without its third level misses small ones (12 of the 200 in review): the third level reaches them.
        self.assertFalse(grid.reachable([292, 216, 14, 13], "g20", *SCREEN, levels=2))

    # TEST: TOOL-U-074
    def test_a_window_holds_a_target_when_they_overlap(self):
        self.assertTrue(grid.overlaps((300, 460, 200, 200), [480, 640, 30, 30]))
        self.assertFalse(grid.overlaps((300, 460, 200, 200), [500, 460, 10, 10]), "touching the right edge only")


class Drawing(unittest.TestCase):
    def screen(self, width=1280, height=800):
        return pngio.Image(width, height, bytearray([240, 240, 240] * width * height))

    # TEST: TOOL-U-074
    def test_the_png_round_trip(self):
        image = pngio.Image(3, 2, bytearray(range(18)))
        again = pngio.decode(pngio.encode(image))
        self.assertEqual((again.width, again.height, bytes(again.rgb)), (3, 2, bytes(range(18))))

    # TEST: TOOL-U-074
    def test_the_decoder_undoes_every_png_filter(self):
        # Filters 1-4 per row (sub, up, average, paeth), as a real encoder may choose them.
        image = pngio.Image(4, 5, bytearray(random.Random(7).randrange(256) for _ in range(60)))
        for filtered in pngio.encode_filtered(image):
            self.assertEqual(bytes(pngio.decode(filtered).rgb), bytes(image.rgb))

    # TEST: TOOL-U-074
    def test_level_1_is_drawn_at_the_screens_size_with_lines_and_a_label_inside_each_cell(self):
        view = grid.level1(*SCREEN)
        drawn, boxes = grid.render(self.screen(), view)
        self.assertEqual((drawn.width, drawn.height), SCREEN)
        self.assertEqual(len(boxes), 40)
        for label, (x, y, w, h) in boxes.items():
            cx, cy, cw, ch = grid.cell_rect(view, label)
            self.assertTrue(cx <= x and cy <= y and x + w <= cx + cw and y + h <= cy + ch, label)
        self.assertEqual(drawn.pixel(160, 400), grid.LINE, "a line on each cell boundary")
        self.assertEqual(drawn.pixel(100, 100), (240, 240, 240), "the screen elsewhere")

    # TEST: TOOL-U-074
    def test_a_zoom_is_drawn_at_640_with_readable_labels_inside_their_cells(self):
        g12 = grid.zoom(grid.level1(*SCREEN), "C4", "g12")
        drawn, boxes = grid.render(self.screen(), g12)
        self.assertEqual((drawn.width, drawn.height), (640, 640))
        self.assertEqual(len(boxes), 225)
        side = 640 / 15
        for label, (x, y, w, h) in boxes.items():
            col, row = grid.parse(g12, label)
            self.assertTrue(col * side <= x and x + w <= (col + 1) * side + 1, label)
            self.assertGreaterEqual(h, 2 * 5, "glyphs at least 2 px a font pixel")
        level3 = grid.zoom(grid.zoom(grid.level1(*SCREEN), "C4", "g20"), "F7", "g20")
        self.assertEqual(len(grid.render(self.screen(), level3)[1]), 16)

    # TEST: TOOL-U-074
    def test_every_label_glyph_is_in_the_font(self):
        for character in grid.COLUMNS[:15] + "0123456789":
            self.assertIn(character, grid.FONT)
            self.assertEqual(len(grid.FONT[character]), 5, character)
            self.assertTrue(all(len(row) == 3 for row in grid.FONT[character]), character)


class Words(unittest.TestCase):
    # TEST: TOOL-U-074
    def test_the_result_words_for_each_level(self):
        top = grid.level1(*SCREEN)
        self.assertEqual(grid.words(top, "g20"),
                         "Grid 1280×800: 8 columns A to H, 5 rows 1 to 5, each cell 160 px. Call grid with the cell "
                         "holding what you want, or click a cell.")
        self.assertEqual(grid.words(grid.zoom(top, "C4", "g20"), "g20"),
                         "Cell C4, 3.2 times closer: 10 columns A to K (no I), 10 rows 1 to 10, each cell 20 px on the "
                         "screen. Click the cell holding the middle of what you want. If what you want is smaller than "
                         "a cell, call grid with that cell to look closer once more.")
        self.assertEqual(grid.words(grid.zoom(grid.zoom(top, "C4", "g20"), "F7", "g20"), "g20"),
                         "Cell F7 of C4, 16 times closer: 4 columns A to D, 4 rows 1 to 4, each cell 10 px on the "
                         "screen. Click the cell holding the middle of what you want.")
        self.assertEqual(grid.words(grid.zoom(top, "C4", "g12"), "g12"),
                         "Cell C4, 3.6 times closer: 15 columns A to Q (no I or O), 15 rows 1 to 15, each cell 12 px "
                         "on the screen. Click the cell holding the middle of what you want.")

    # TEST: TOOL-U-074
    def test_the_tool_spec_adds_grid_and_cell_to_the_mark_routes_recorded_spec(self):
        recorded = {"name": "computer", "description": "Operate the desktop.",
                    "parameters": {"type": "object", "additionalProperties": False,
                                   "required": ["action", "ref"],
                                   "properties": {"action": {"type": "string",
                                                             "enum": ["observe", "screenshot", "click", "type"]},
                                                  "ref": {"type": ["string", "null"]}}}}
        spec = grid.tool_spec(recorded)
        props = spec["parameters"]["properties"]
        self.assertEqual(props["action"]["enum"], ["observe", "screenshot", "grid", "click", "type"])
        self.assertEqual(props["cell"]["type"], ["string", "null"])
        self.assertIn("cell", spec["parameters"]["required"])
        self.assertIn("call grid", spec["description"])
        self.assertNotIn("grid", recorded["parameters"]["properties"]["action"]["enum"], "the record unchanged")


if __name__ == "__main__":
    unittest.main()
