import unittest
from shop import refund, split
from shop.money import Money


class RefundSplitTests(unittest.TestCase):
    def test_split_refund_sums_to_paid(self):
        parts = refund.split_refund(Money("100.00"), 3)
        self.assertEqual(sum((p for p in parts), Money.zero()), Money("100.00"))

    def test_split_by_share(self):
        parts = split.split_by_share(Money("0.05"), [1, 1])
        self.assertEqual([str(p) for p in parts], ["0.03 EUR", "0.02 EUR"])
