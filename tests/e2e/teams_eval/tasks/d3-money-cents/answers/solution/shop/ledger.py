from shop.util import total


class Ledger:
    def __init__(self, currency="EUR"):
        self.currency, self.entries = currency, []

    def post(self, money, memo):
        self.entries.append((money, memo))

    def balance(self):
        return total([money for money, _ in self.entries], self.currency)
