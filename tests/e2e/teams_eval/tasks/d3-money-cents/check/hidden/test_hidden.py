import ast
import unittest
from decimal import Decimal
from pathlib import Path

from shop import cart, fx, ledger, refund, report, split, tax
from shop.money import CurrencyMismatch, Money


class Cents(unittest.TestCase):
    def test_money_holds_integer_cents(self):
        money = Money("12.34")
        self.assertIsInstance(money.cents, int)
        self.assertEqual(money.cents, 1234)
        self.assertEqual(Money("0.1").cents, 10)

    def test_amount_is_exact(self):
        self.assertEqual(Money("0.10").amount + Money("0.20").amount, Money("0.30").amount)
        self.assertEqual(Decimal(str(Money("19.99").amount)), Decimal("19.99"))

    def test_no_float_arithmetic_on_money_in_the_package(self):
        found = []
        for path in Path("shop").glob("*.py"):
            tree = ast.parse(path.read_text())
            for node in ast.walk(tree):
                if isinstance(node, ast.Call) and getattr(node.func, "id", None) == "float":
                    found.append(f"{path}:{node.lineno} float()")
                if isinstance(node, ast.Constant) and isinstance(node.value, float):
                    found.append(f"{path}:{node.lineno} {node.value}")
        self.assertEqual(found, [], "money as float")

    def test_more_exact_cases(self):
        c = cart.Cart()
        c.add(Money("0.07"), 7)
        c.add(Money("33.33"), 3, "reduced")
        self.assertEqual(str(c.gross()), "107.57 EUR")
        self.assertEqual(str(tax.tax(Money("0.05"))), "0.01 EUR")
        self.assertEqual([str(p) for p in split.split_bill(Money("1.00"), 6)],
                         ["0.17 EUR", "0.17 EUR", "0.17 EUR", "0.17 EUR", "0.16 EUR", "0.16 EUR"])
        self.assertEqual(str(refund.partial(Money("9.99"), Decimal("0.5"))), "5.00 EUR")
        self.assertEqual(str(fx.convert(Money("100.00"), "GBP")), "84.12 GBP")
        self.assertEqual(report.average([Money("0.01"), Money("0.02")]), Money("0.02").amount)

    def test_the_public_api_is_unchanged(self):
        self.assertEqual(str(Money.parse("1.00 USD") + Money("2.50", "USD")), "3.50 USD")
        with self.assertRaises(CurrencyMismatch):
            Money("1.00") + Money("1.00", "USD")
        self.assertTrue(Money("1.00") < Money("1.01"))
        self.assertEqual(len({Money("1.00"), Money("1.0")}), 1)
        book = ledger.Ledger()
        book.post(Money("0.10"), "a")
        book.post(Money("0.20"), "b")
        self.assertEqual(book.balance(), Money("0.30"))
