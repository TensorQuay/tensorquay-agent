import datetime
import unittest

from lib import dates


class DatesTest(unittest.TestCase):
    def test_business_days(self):
        friday = dates.parse_date("2026-10-02")
        self.assertTrue(dates.is_weekend(dates.parse_date("2026-10-03")))
        self.assertEqual(dates.add_business_days(friday, 1), datetime.date(2026, 10, 5))
        self.assertEqual(dates.add_business_days(friday, -1), datetime.date(2026, 10, 1))

    def test_quarter_and_month_range(self):
        self.assertEqual(dates.quarter(datetime.date(2026, 8, 1)), 3)
        self.assertEqual(dates.month_range(2026, 2), (datetime.date(2026, 2, 1), datetime.date(2026, 2, 28)))
        self.assertEqual(dates.month_range(2026, 12)[1], datetime.date(2026, 12, 31))
