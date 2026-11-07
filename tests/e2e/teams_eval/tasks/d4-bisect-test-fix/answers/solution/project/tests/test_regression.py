import unittest
from textkit.wrap import wrap


class Regression(unittest.TestCase):
    def test_a_line_exactly_width_long_stays_whole(self):
        self.assertEqual(wrap("aaaa bbbb", 9), ["aaaa bbbb"])
        self.assertEqual(wrap("aa bb cc dd", 5), ["aa bb", "cc dd"])
