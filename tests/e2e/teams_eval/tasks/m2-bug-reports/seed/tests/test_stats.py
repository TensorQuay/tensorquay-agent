import unittest

from calc import stats


class StatsTest(unittest.TestCase):
    def test_mean(self):
        self.assertEqual(stats.mean([1, 2, 3]), 2)
