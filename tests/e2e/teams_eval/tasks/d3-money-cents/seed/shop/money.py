"""Money in one currency. Public API: Money(amount, currency="EUR"), Money.parse(text),
Money.zero(currency),
+ and - between Moneys of one currency, * by a number, allocate(ratios), comparisons, str() as "12.34 EUR", and
.amount (the amount as a number)."""


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
