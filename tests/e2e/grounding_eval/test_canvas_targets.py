"""The grounding evaluation's canvas set: 60 targets with no element of their own, on six pages, the same every time;
each box inside its surface, with a size, apart from its page's others; each page with its anchor heading and a spec
the drawing reads; and merged after the 200 only when asked, so the 200's file stays as it was.

Run: python3 -m unittest tests/e2e/grounding_eval/test_canvas_targets.py"""

import json
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ElementTree
from html.parser import HTMLParser
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import canvas_targets  # noqa: E402
import targets  # noqa: E402


class Page(HTMLParser):
    """A canvas page's heading text, its spec's JSON and its surface's size."""

    def __init__(self):
        super().__init__()
        self.heading, self.spec, self.surface, self.open = "", None, None, None

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        self.open = "spec" if attrs.get("id") == "spec" else tag
        if tag in ("canvas", "img"):
            self.surface = (int(attrs["width"]), int(attrs["height"]))

    def handle_data(self, data):
        if self.open == "h1":
            self.heading += data
        elif self.open == "spec":
            self.spec = (self.spec or "") + data

    def handle_endtag(self, tag):
        self.open = None


def parsed(text):
    page = Page()
    page.feed(text)
    return page


class CanvasSet(unittest.TestCase):
    # TEST: TOOL-U-072
    def test_sixty_targets_ten_a_page_the_same_every_time(self):
        files, chosen = canvas_targets.build()
        canvas_targets.check(files, chosen)
        self.assertEqual(len(chosen), 60)
        pages = sorted({item["page"] for item in chosen})
        self.assertEqual(pages, [f"canvas-{n:02d}" for n in range(1, 7)])
        self.assertTrue(all(sum(1 for item in chosen if item["page"] == page) == 10 for page in pages))
        self.assertEqual(len({item["id"] for item in chosen}), 60)
        self.assertTrue(all(item["kind"] == "canvas" for item in chosen))
        self.assertEqual(canvas_targets.build(), canvas_targets.build())

    # TEST: TOOL-U-072
    def test_each_box_is_inside_its_surface_with_a_size_and_apart_from_its_pages_others(self):
        files, chosen = canvas_targets.build()
        left, top = canvas_targets.SURFACE_AT
        for page in sorted({item["page"] for item in chosen}):
            surface = parsed(files[f"{page}.html"]).surface
            boxes = [item["box_in_anchor"] for item in chosen if item["page"] == page]
            for x, y, width, height in boxes:
                self.assertGreaterEqual(min(width, height), 12, page)
                self.assertTrue(left <= x and top <= y and x + width <= left + surface[0]
                                and y + height <= top + surface[1], (page, x, y, width, height))
            for index, box in enumerate(boxes):
                for other in boxes[:index]:
                    self.assertFalse(canvas_targets.overlaps(box, other), (page, box, other))

    # TEST: TOOL-U-072
    def test_each_page_has_its_anchor_heading_and_a_spec_the_drawing_reads(self):
        files, chosen = canvas_targets.build()
        for item in chosen:
            page = parsed(files[f"{item['page']}.html"])
            self.assertEqual(item["anchor"], {"role": "heading", "name": page.heading}, item["id"])
            self.assertIn(item["anchor"]["name"].rsplit(" ", 1)[-1], f"({item['page']})")
        drawn = [name for name in files if name.endswith(".html") and parsed(files[name]).spec is not None]
        self.assertEqual(len(drawn), 5, "five canvases; the picture page draws nothing")
        for name in drawn:
            spec = json.loads(parsed(files[name]).spec)
            self.assertTrue(spec["shapes"], name)
            self.assertNotIn("</", parsed(files[name]).spec, "the spec can't close its script early")
        picture = ElementTree.fromstring(files["canvas-05.svg"])
        self.assertEqual(picture.tag, "{http://www.w3.org/2000/svg}svg")
        self.assertEqual(len(picture.findall("{http://www.w3.org/2000/svg}rect")), 11, "a background and 10 buttons")

    # TEST: TOOL-U-072
    def test_a_bad_set_is_refused(self):
        files, chosen = canvas_targets.build()
        tiny = [dict(item) for item in chosen]
        tiny[0]["box_in_anchor"] = tiny[0]["box_in_anchor"][:2] + [8, 8]
        with self.assertRaises(AssertionError):
            canvas_targets.check(files, tiny)
        with self.assertRaises(AssertionError):
            canvas_targets.check(files, chosen[:-1])


class Merged(unittest.TestCase):
    def write(self, *flags):
        out = Path(tempfile.mkdtemp())
        targets.main([str(out), *flags])
        return out, json.loads((out / "targets.json").read_text(encoding="utf-8"))

    # TEST: TOOL-U-072
    def test_the_200_alone_by_default_and_the_260_with_canvas(self):
        plain_dir, plain = self.write()
        self.assertEqual(len(plain["targets"]), 200)
        self.assertNotIn("canvas", plain)
        self.assertFalse(list((plain_dir / "pages").glob("canvas-*")))
        merged_dir, merged = self.write("--canvas")
        self.assertEqual(len(merged["targets"]), 260)
        self.assertEqual(merged["targets"][:200], plain["targets"], "the 200 first, unchanged")
        self.assertEqual(merged["canvas"], {"seed": canvas_targets.SEED, "count": 60})
        self.assertEqual({k: v for k, v in merged.items() if k not in ("targets", "canvas")},
                         {k: v for k, v in plain.items() if k != "targets"})
        names = sorted(path.name for path in (merged_dir / "pages").glob("canvas-*"))
        self.assertEqual(names, sorted([f"canvas-{n:02d}.html" for n in range(1, 7)] + ["canvas-05.svg"]))

    # TEST: TOOL-U-072
    def test_the_default_file_is_byte_for_byte_as_before(self):
        # The 200's file is what every earlier run compared against (12 and 15 Oct): the flag must not change it.
        out = Path(tempfile.mkdtemp())
        targets.main([str(out)])
        before = Path(tempfile.mkdtemp())
        pages, chosen = targets.build()
        desktop = sorted({item["page"] for item in chosen if item["kind"] == "desktop"})
        expected = json.dumps({"seed": targets.SEED, "screen": targets.SCREEN, "desktop": desktop,
                               "targets": chosen}, ensure_ascii=False, indent=1)
        (before / "targets.json").write_text(expected, encoding="utf-8")
        self.assertEqual((out / "targets.json").read_bytes(), (before / "targets.json").read_bytes())


if __name__ == "__main__":
    unittest.main()
