from shop.money import Money


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
