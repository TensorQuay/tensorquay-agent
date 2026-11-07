import importlib
import json
import unittest

EXPECTED = json.load(open("/check/expected.json"))


class HeldOutTest(unittest.TestCase):
    def test_zero_requests_return_zero(self):
        broken = importlib.import_module("app.handlers.h27")
        self.assertEqual(broken.ratio(5, 0), 0.0)
        self.assertEqual(broken.ratio(6, 3), 2.0)

    def test_every_handler_behaves_as_before(self):
        for name, expected in EXPECTED.items():
            module = importlib.import_module(f"app.handlers.{name}")
            self.assertEqual(module.score([1, 2, 3]), expected["score"], name)
            self.assertEqual(module.ratio(9, 4), expected["ratio"], name)
            self.assertEqual(module.ratio(1, 0), 0.0, name)
