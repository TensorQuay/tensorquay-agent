import unittest

from units import to_celsius, to_fahrenheit


class HeldOutTest(unittest.TestCase):
    def test_round_trip(self):
        for celsius in (-40, -10, 0, 37, 100):
            self.assertAlmostEqual(to_celsius(to_fahrenheit(celsius)), celsius)

    def test_minus_forty(self):
        self.assertEqual(to_fahrenheit(-40), -40)
