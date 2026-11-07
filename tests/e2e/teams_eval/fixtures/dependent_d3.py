"""The agent-teams evaluation's D3 task (dependent, docs/evals/agent-teams.md section 2)."""


from common import Task

TEST_COMMAND = "python3 -m unittest discover -s tests -t ."

# D3: Money from float to integer cents across a package whose modules call each other and share a helper, keeping
# the public API. Parallel edits to the shared modules collide unless they're ordered or merged.
D3_MONEY_SEED = """\"\"\"Money in one currency. Public API: Money(amount, currency="EUR"), Money.parse(text),
Money.zero(currency),
+ and - between Moneys of one currency, * by a number, allocate(ratios), comparisons, str() as "12.34 EUR", and
.amount (the amount as a number).\"\"\"


class CurrencyMismatch(ValueError):
    pass


class Money:
    def __init__(self, amount, currency="EUR"):
        self.amount = float(amount)
        self.currency = currency

    @classmethod
    def parse(cls, text):
        amount, currency = text.split()
        return cls(amount, currency)

    @classmethod
    def zero(cls, currency="EUR"):
        return cls(0, currency)

    def _same(self, other):
        if self.currency != other.currency:
            raise CurrencyMismatch(f"{self.currency} and {other.currency}")

    def __add__(self, other):
        self._same(other)
        return Money(self.amount + other.amount, self.currency)

    def __sub__(self, other):
        self._same(other)
        return Money(self.amount - other.amount, self.currency)

    def __mul__(self, factor):
        return Money(round(self.amount * float(factor), 2), self.currency)

    __rmul__ = __mul__

    def allocate(self, ratios):
        total = sum(ratios)
        return [Money(round(self.amount * ratio / total, 2), self.currency) for ratio in ratios]

    def __eq__(self, other):
        return isinstance(other, Money) and (self.amount, self.currency) == (other.amount, other.currency)

    def __lt__(self, other):
        self._same(other)
        return self.amount < other.amount

    def __le__(self, other):
        return self == other or self < other

    def __hash__(self):
        return hash((self.amount, self.currency))

    def __str__(self):
        return f"{self.amount:.2f} {self.currency}"

    __repr__ = __str__
"""

# The modules: each uses Money, several reach into .amount with float arithmetic, and five share util.round2.
D3_MODULES = {
    "shop/util.py": """from shop.money import Money


def round2(value):
    return round(value, 2)


def total(moneys, currency="EUR"):
    result = Money.zero(currency)
    for money in moneys:
        result = result + money
    return result
""",
    "shop/tax.py": """from shop.money import Money
from shop.util import round2

RATES = {"standard": 0.19, "reduced": 0.07, "none": 0.0}


def tax(net, kind="standard"):
    return Money(round2(net.amount * RATES[kind]), net.currency)


def gross(net, kind="standard"):
    return net + tax(net, kind)
""",
    "shop/discount.py": """from shop.money import Money
from shop.util import round2


def percent_off(price, percent):
    return Money(round2(price.amount * (100 - percent) / 100), price.currency)


def amount_off(price, off):
    return price - off if off < price else Money.zero(price.currency)
""",
    "shop/cart.py": """from shop import discount, tax
from shop.util import total


class Cart:
    def __init__(self, currency="EUR"):
        self.currency, self.lines = currency, []

    def add(self, price, quantity=1, kind="standard"):
        self.lines.append((price, quantity, kind))

    def net(self):
        return total([price * quantity for price, quantity, _ in self.lines], self.currency)

    def tax(self):
        return total([tax.tax(price * quantity, kind) for price, quantity, kind in self.lines], self.currency)

    def gross(self):
        return self.net() + self.tax()

    def with_percent_off(self, percent):
        return discount.percent_off(self.gross(), percent)
""",
    "shop/invoice.py": """from shop.cart import Cart


def lines(cart: Cart):
    return [f"{quantity} x {price} = {price * quantity}" for price, quantity, _ in cart.lines]


def summary(cart: Cart):
    return {"net": str(cart.net()), "tax": str(cart.tax()), "gross": str(cart.gross())}
""",
    "shop/refund.py": """from shop.money import Money
from shop.util import round2


def partial(paid, fraction):
    return Money(round2(paid.amount * fraction), paid.currency)


def split_refund(paid, parts):
    return paid.allocate([1] * parts)
""",
    "shop/ledger.py": """from shop.money import Money


class Ledger:
    def __init__(self, currency="EUR"):
        self.currency, self.entries = currency, []

    def post(self, money, memo):
        self.entries.append((money, memo))

    def balance(self):
        value = 0.0
        for money, _ in self.entries:
            value += money.amount
        return Money(value, self.currency)
""",
    "shop/fx.py": """from shop.money import Money
from shop.util import round2

RATES = {("EUR", "USD"): 1.0865, ("USD", "EUR"): 0.9204, ("EUR", "GBP"): 0.8412}


def convert(money, currency):
    if currency == money.currency:
        return money
    return Money(round2(money.amount * RATES[(money.currency, currency)]), currency)
""",
    "shop/split.py": """def split_bill(total, people):
    return total.allocate([1] * people)


def split_by_share(total, shares):
    return total.allocate(shares)
""",
    "shop/loyalty.py": """def points(gross):
    return int(gross.amount)
""",
    "shop/shipping.py": """from shop.money import Money

FREE_FROM = Money("50.00")


def shipping(gross):
    return Money.zero(gross.currency) if not gross < FREE_FROM else Money("4.95", gross.currency)
""",
    "shop/report.py": """from shop.util import round2


def average(moneys):
    return round2(sum(money.amount for money in moneys) / len(moneys))


def line(name, money):
    return f"{name:<12}{money.amount:>10.2f} {money.currency}"
""",
}

D3_TESTS = {
    "tests/test_money.py": """import unittest
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
""",
    "tests/test_cart.py": """import unittest
from shop.cart import Cart
from shop.money import Money


class CartTests(unittest.TestCase):
    def test_gross_of_many_small_lines(self):
        cart = Cart()
        for _ in range(10):
            cart.add(Money("0.10"), 3)
        self.assertEqual(str(cart.net()), "3.00 EUR")

    def test_tax_and_gross(self):
        cart = Cart()
        cart.add(Money("19.99"), 3)
        cart.add(Money("2.49"), 1, "reduced")
        self.assertEqual(str(cart.tax()), "11.56 EUR")
        self.assertEqual(str(cart.gross()), "74.02 EUR")
""",
    "tests/test_ledger.py": """import unittest
from shop.ledger import Ledger
from shop.money import Money


class LedgerTests(unittest.TestCase):
    def test_balance_after_many_postings(self):
        ledger = Ledger()
        for _ in range(1000):
            ledger.post(Money("0.01"), "fee")
        ledger.post(Money("-5.00"), "refund")
        self.assertEqual(str(ledger.balance()), "5.00 EUR")
""",
    "tests/test_refund_split.py": """import unittest
from shop import refund, split
from shop.money import Money


class RefundSplitTests(unittest.TestCase):
    def test_split_refund_sums_to_paid(self):
        parts = refund.split_refund(Money("100.00"), 3)
        self.assertEqual(sum((p for p in parts), Money.zero()), Money("100.00"))

    def test_split_by_share(self):
        parts = split.split_by_share(Money("0.05"), [1, 1])
        self.assertEqual([str(p) for p in parts], ["0.03 EUR", "0.02 EUR"])
""",
    "tests/test_fx_report.py": """import unittest
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
""",
}

D3_HIDDEN = """import ast
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
"""

D3_SOLUTION = {
    "shop/money.py": """\"\"\"Money in one currency, held as integer cents. Public API: Money(amount, currency="EUR"),
Money.parse(text),
Money.zero(currency), + and - between Moneys of one currency, * by a number, allocate(ratios), comparisons, str() as
"12.34 EUR", .amount (the amount as an exact Decimal) and .cents.\"\"\"
from decimal import ROUND_HALF_UP, Decimal


class CurrencyMismatch(ValueError):
    pass


def _cents(amount):
    return int((Decimal(str(amount)) * 100).quantize(Decimal("1"), rounding=ROUND_HALF_UP))


class Money:
    def __init__(self, amount, currency="EUR"):
        self.cents = _cents(amount)
        self.currency = currency

    @classmethod
    def of_cents(cls, cents, currency="EUR"):
        money = cls(0, currency)
        money.cents = int(cents)
        return money

    @property
    def amount(self):
        return Decimal(self.cents) / 100

    @classmethod
    def parse(cls, text):
        amount, currency = text.split()
        return cls(amount, currency)

    @classmethod
    def zero(cls, currency="EUR"):
        return cls(0, currency)

    def _same(self, other):
        if self.currency != other.currency:
            raise CurrencyMismatch(f"{self.currency} and {other.currency}")

    def __add__(self, other):
        self._same(other)
        return Money.of_cents(self.cents + other.cents, self.currency)

    def __sub__(self, other):
        self._same(other)
        return Money.of_cents(self.cents - other.cents, self.currency)

    def __mul__(self, factor):
        return Money(self.amount * Decimal(str(factor)), self.currency)

    __rmul__ = __mul__

    def allocate(self, ratios):
        total = sum(ratios)
        shares = [self.cents * ratio // total for ratio in ratios]
        for index in range(self.cents - sum(shares)):
            shares[index] += 1
        return [Money.of_cents(share, self.currency) for share in shares]

    def __eq__(self, other):
        return isinstance(other, Money) and (self.cents, self.currency) == (other.cents, other.currency)

    def __lt__(self, other):
        self._same(other)
        return self.cents < other.cents

    def __le__(self, other):
        return self == other or self < other

    def __hash__(self):
        return hash((self.cents, self.currency))

    def __str__(self):
        return f"{self.amount:.2f} {self.currency}"

    __repr__ = __str__
""",
    "shop/util.py": """from shop.money import Money


def total(moneys, currency="EUR"):
    result = Money.zero(currency)
    for money in moneys:
        result = result + money
    return result
""",
    "shop/tax.py": """from decimal import Decimal

RATES = {"standard": Decimal("0.19"), "reduced": Decimal("0.07"), "none": Decimal("0")}


def tax(net, kind="standard"):
    return net * RATES[kind]


def gross(net, kind="standard"):
    return net + tax(net, kind)
""",
    "shop/discount.py": """from decimal import Decimal

from shop.money import Money


def percent_off(price, percent):
    return price * ((Decimal(100) - Decimal(str(percent))) / 100)


def amount_off(price, off):
    return price - off if off < price else Money.zero(price.currency)
""",
    "shop/refund.py": """from decimal import Decimal


def partial(paid, fraction):
    return paid * Decimal(str(fraction))


def split_refund(paid, parts):
    return paid.allocate([1] * parts)
""",
    "shop/ledger.py": """from shop.util import total


class Ledger:
    def __init__(self, currency="EUR"):
        self.currency, self.entries = currency, []

    def post(self, money, memo):
        self.entries.append((money, memo))

    def balance(self):
        return total([money for money, _ in self.entries], self.currency)
""",
    "shop/fx.py": """from decimal import Decimal

from shop.money import Money

RATES = {("EUR", "USD"): Decimal("1.0865"), ("USD", "EUR"): Decimal("0.9204"), ("EUR", "GBP"): Decimal("0.8412")}


def convert(money, currency):
    if currency == money.currency:
        return money
    return Money(money.amount * RATES[(money.currency, currency)], currency)
""",
    "shop/report.py": """from decimal import ROUND_HALF_UP, Decimal


def average(moneys):
    return (sum((money.amount for money in moneys), Decimal(0)) / len(moneys)).quantize(Decimal("0.01"),
                                                                                         rounding=ROUND_HALF_UP)


def line(name, money):
    return f"{name:<12}{money.amount:>10.2f} {money.currency}"
""",
}


def d3():
    seed = {"shop/__init__.py": "", "shop/money.py": D3_MONEY_SEED, "tests/__init__.py": "",
            "README.md": f"# shop\n\nA shop's money code. Tests: `{TEST_COMMAND}`.\n"}
    seed.update(D3_MODULES)
    seed.update(D3_TESTS)
    solution = dict(D3_SOLUTION)
    # Half done: Money holds cents, but the modules that reached into .amount still do float arithmetic on it.
    wrong = {"shop/money.py": D3_SOLUTION["shop/money.py"]}
    check = """
from checklib import fail, held_out, run, visible

def check(workspace):
    visible(workspace)
    held_out(workspace)
    return "Money is integer cents throughout the package, its public API unchanged, and every case exact"

run(check)
"""
    return Task(
        id="d3-money-cents", title="Move Money to integer cents across a package, keeping its API", kind="dependent",
        statement=f"""
The package in `shop/` keeps money as floating point, and some of its tests fail on rounding. Change `Money` to hold
integer cents (as a `cents` attribute) everywhere in the package, so that no module does float arithmetic on money,
while keeping `Money`'s public API (the names in its docstring) and every module's function names and arguments.
`amount` may become an exact decimal. Don't change the tests; run them with `{TEST_COMMAND}`.
""",
        seed=seed, check=check, budgets=(80, 2100, 1_000_000), check_timeout=120,
        protected=sorted(D3_TESTS) + ["tests/__init__.py"], hidden={"test_hidden.py": D3_HIDDEN},
        key_facts=["Money holds integer cents; amount is an exact Decimal (or derived from cents)",
                   "Every module that did float arithmetic on .amount (tax, discount, refund, ledger, fx, report, and "
                   "the shared util.round2) now goes through Money or Decimal",
                   "Multiplication rounds half up to the cent; allocate gives the remainder cents to the first parts",
                   "The public API and the tests are unchanged"],
        solution=solution, wrong=wrong)
