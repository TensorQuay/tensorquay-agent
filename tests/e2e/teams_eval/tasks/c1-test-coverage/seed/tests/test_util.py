import unittest

from lib.util import clamp


class UtilTest(unittest.TestCase):
    def test_clamp(self):
        self.assertEqual(clamp(5, 0, 3), 3)
        self.assertEqual(clamp(-1, 0, 3), 0)
