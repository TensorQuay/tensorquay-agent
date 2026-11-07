import unittest

from calc import stats


class Issues(unittest.TestCase):
    def test_101_percent_keeps_two_places(self):
        self.assertEqual(stats.percent(1, 3), 33.33)

    def test_103_median_of_unsorted_values(self):
        self.assertEqual(stats.median([3, 1, 2]), 2)
        self.assertEqual(stats.median([4, 1, 3, 2]), 2.5)

    def test_102_mean_of_nothing_is_zero(self):
        # Not a bug: mean() documents a ValueError for no values.
        self.assertEqual(stats.mean([]), 0)
