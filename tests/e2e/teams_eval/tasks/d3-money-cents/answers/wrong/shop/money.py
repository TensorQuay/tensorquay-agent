"""Money in one currency, held as integer cents. Public API: Money(amount, currency="EUR"),
Money.parse(text),
Money.zero(currency), + and - between Moneys of one currency, * by a number, allocate(ratios), comparisons, str() as
"12.34 EUR", .amount (the amount as an exact Decimal) and .cents."""
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
