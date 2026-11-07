import importlib
import json
import unittest
from pathlib import Path

EXPECTED = json.loads((Path(__file__).resolve().parent.parent / "expected.json").read_text())


class HeldOut(unittest.TestCase):
    def test_the_two_crashes_are_fixed(self):
        self.assertEqual(importlib.import_module("app.handlers.h27").ratio(5, 0), 0.0)
        self.assertEqual(importlib.import_module("app.handlers.h13").score([]), 0)

    def test_every_handler_behaves_as_before(self):
        for name, expected in EXPECTED.items():
            module = importlib.import_module(f"app.handlers.{name}")
            self.assertEqual(module.score([1, 2, 3]), expected["score"], name)
            self.assertEqual(module.ratio(9, 4), expected["ratio"], name)
            self.assertEqual(module.ratio(1, 0), 0.0, name)
            self.assertEqual(module.score([]), 0, name)
