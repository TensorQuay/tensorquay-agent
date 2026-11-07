"""The grounding evaluation's target set: 200 targets in the planned kinds, the same every time, each unique on its
screen by role and name, on pages that name every target as its own element.

Run: python3 -m unittest tests/e2e/grounding_eval/test_targets.py"""

import sys
import unittest
from html.parser import HTMLParser
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import targets  # noqa: E402


class Names(HTMLParser):
    """Every accessible name a page gives: aria-labels and button, link and menu item texts."""

    def __init__(self):
        super().__init__()
        self.names, self.open = set(), None

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if "aria-label" in attrs:
            self.names.add(attrs["aria-label"])
        self.open = tag if tag in ("button", "a") or attrs.get("role") == "menuitem" else None

    def handle_data(self, data):
        if self.open and data.strip():
            self.names.add(data.strip())


class TargetSet(unittest.TestCase):
    def test_the_set_has_its_planned_kinds_and_rules(self):
        pages, chosen = targets.build()
        targets.check(pages, chosen)
        self.assertEqual(len(chosen), 200)
        self.assertEqual({kind: sum(1 for item in chosen if item["kind"] == kind) for kind in targets.KINDS},
                         targets.KINDS)

    def test_it_is_the_same_every_time(self):
        self.assertEqual(targets.build(), targets.build())

    def test_every_page_target_is_named_on_its_page(self):
        pages, chosen = targets.build()
        for item in chosen:
            if item["kind"] == "desktop":
                continue
            parser = Names()
            parser.feed(pages[item["page"]])
            self.assertIn(item["name"], parser.names, item["id"])

    def test_the_chinese_and_dark_pages_are_what_they_say(self):
        pages, chosen = targets.build()
        chinese = [item for item in chosen if item["kind"] == "chinese"]
        self.assertTrue(all(any("一" <= char <= "鿿" for char in item["instruction"]) for item in chinese))
        dark = {item["page"] for item in chosen if item["kind"] == "dark"}
        self.assertTrue(all("#111418" in pages[page] for page in dark))


if __name__ == "__main__":
    unittest.main()
