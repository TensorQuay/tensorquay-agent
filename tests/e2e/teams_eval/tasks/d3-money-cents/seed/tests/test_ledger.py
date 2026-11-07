import unittest
from shop.ledger import Ledger
from shop.money import Money


class LedgerTests(unittest.TestCase):
    def test_balance_after_many_postings(self):
        ledger = Ledger()
        for _ in range(1000):
            ledger.post(Money("0.01"), "fee")
        ledger.post(Money("-5.00"), "refund")
        self.assertEqual(str(ledger.balance()), "5.00 EUR")
