import unittest

from web.helpers_01 import clamp_1, pairs_1


class Helpers(unittest.TestCase):
    def test_clamp(self):
        self.assertEqual(clamp_1(50), 11)

    def test_pairs(self):
        self.assertEqual(pairs_1('abc'), [('a', 'b'), ('b', 'c')])
