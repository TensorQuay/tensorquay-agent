import unittest

from calc import stats


class HeldOut(unittest.TestCase):
    def test_percent(self):
        self.assertEqual(stats.percent(2, 3), 66.67)
        self.assertEqual(stats.percent(1, 4), 25.0)

    def test_median(self):
        self.assertEqual(stats.median([9, 7, 8, 1, 5]), 7)
        self.assertEqual(stats.median([10, 2]), 6)

    def test_mean_still_refuses_no_values(self):
        with self.assertRaises(ValueError):
            stats.mean([])
        self.assertEqual(stats.mean([2, 4]), 3)
