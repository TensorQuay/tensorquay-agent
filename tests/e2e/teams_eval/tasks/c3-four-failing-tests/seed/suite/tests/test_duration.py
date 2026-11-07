import unittest

from pkg.duration import parse_duration


class DurationTest(unittest.TestCase):
    def test_hours_and_minutes(self):
        self.assertEqual(parse_duration("1h30m"), 5400)
        self.assertEqual(parse_duration("45m"), 2700)
