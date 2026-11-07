import unittest
from textkit.wrap import wrap


class Wrap(unittest.TestCase):
    def test_short(self):
        self.assertEqual(wrap("a b c", 10), ["a b c"])

    def test_breaks(self):
        self.assertEqual(wrap("aaa bbb ccc", 7), ["aaa bbb", "ccc"])
