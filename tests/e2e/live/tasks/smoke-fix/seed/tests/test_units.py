import unittest

from units import to_celsius, to_fahrenheit


class UnitsTest(unittest.TestCase):
    def test_freezing(self):
        self.assertEqual(to_fahrenheit(0), 32)

    def test_boiling(self):
        self.assertEqual(to_fahrenheit(100), 212)

    def test_back(self):
        self.assertEqual(to_celsius(212), 100)
