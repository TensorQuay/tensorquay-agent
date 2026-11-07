import unittest
from shop import fx, loyalty, report, shipping
from shop.money import Money


class OtherTests(unittest.TestCase):
    def test_convert(self):
        self.assertEqual(str(fx.convert(Money("10.00"), "USD")), "10.87 USD")

    def test_report_line(self):
        self.assertEqual(report.line("total", Money("1234.5")), "total          1234.50 EUR")

    def test_points_and_shipping(self):
        self.assertEqual(loyalty.points(Money("49.99")), 49)
        self.assertEqual(str(shipping.shipping(Money("50.00"))), "0.00 EUR")
