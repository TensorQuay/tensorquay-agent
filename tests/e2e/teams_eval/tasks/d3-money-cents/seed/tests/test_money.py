import unittest
from shop.money import Money


class MoneyTests(unittest.TestCase):
    def test_sums_are_exact(self):
        self.assertEqual(str(Money("0.10") + Money("0.20")), "0.30 EUR")
        self.assertEqual(Money("0.10") + Money("0.20"), Money("0.30"))

    def test_multiply_rounds_half_up(self):
        self.assertEqual(str(Money("1.005") * 1), "1.01 EUR")
        self.assertEqual(str(Money("2.675") * 1), "2.68 EUR")

    def test_allocate_sums_to_the_whole(self):
        parts = Money("10.00").allocate([1, 1, 1])
        self.assertEqual([str(part) for part in parts], ["3.34 EUR", "3.33 EUR", "3.33 EUR"])

    def test_parse_and_str(self):
        self.assertEqual(str(Money.parse("12.30 USD")), "12.30 USD")
