import unittest

from pkg.window import moving_average


class WindowTest(unittest.TestCase):
    def test_every_window(self):
        self.assertEqual(moving_average([1, 2, 3, 4], 2), [1.5, 2.5, 3.5])
