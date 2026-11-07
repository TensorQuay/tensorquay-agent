import importlib
import json
import unittest
import warnings

EXPECTED = json.load(open("/check/expected.json"))


class HeldOutTest(unittest.TestCase):
    def test_every_endpoint_returns_the_same_line_without_the_deprecated_client(self):
        for name, expected in EXPECTED.items():
            with warnings.catch_warnings():
                warnings.simplefilter("error", DeprecationWarning)
                module = importlib.import_module(f"app.{name}")
                self.assertEqual(module.run(), expected, name)
