import unittest
from decimal import Decimal

from pkg.duration import parse_duration
from pkg.rounding import round_half_up
from pkg.slug import slugify
from pkg.window import moving_average


class HeldOut(unittest.TestCase):
    def test_duration(self):
        self.assertEqual(parse_duration("2h5m10s"), 7510)
        self.assertEqual(parse_duration("90s"), 90)
        with self.assertRaises(ValueError):
            parse_duration("soon")

    def test_rounding(self):
        self.assertEqual(round_half_up("-2.5"), Decimal("-3"))
        self.assertEqual(round_half_up("2.44", 1), Decimal("2.4"))

    def test_slug(self):
        self.assertEqual(slugify("  --Rust & Python 3--  "), "rust-python-3")

    def test_window(self):
        self.assertEqual(moving_average([2, 4], 2), [3.0])
        self.assertEqual(moving_average([1, 2, 3], 1), [1.0, 2.0, 3.0])
