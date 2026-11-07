import unittest
from decimal import Decimal

from pkg.rounding import round_half_up


class RoundingTest(unittest.TestCase):
    def test_half_goes_up(self):
        self.assertEqual(round_half_up("2.5"), Decimal("3"))
        self.assertEqual(round_half_up("1.005", 2), Decimal("1.01"))
