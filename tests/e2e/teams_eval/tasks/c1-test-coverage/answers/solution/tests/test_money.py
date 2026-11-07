import unittest

from lib import money


class MoneyTest(unittest.TestCase):
    def test_parse(self):
        self.assertEqual(money.parse("12.34"), 1234)
        self.assertEqual(money.parse("-0.5"), -50)
        for bad in ("", "1.2.3", "abc"):
            with self.assertRaises(ValueError):
                money.parse(bad)

    def test_format(self):
        self.assertEqual(money.format_cents(123456), "1,234.56 EUR")
        self.assertEqual(money.format_cents(-5), "-0.05 EUR")

    def test_split_and_vat(self):
        self.assertEqual(money.split(100, 3), [34, 33, 33])
        with self.assertRaises(ValueError):
            money.split(1, 0)
        self.assertEqual(money.add_vat(1000), 1200)
