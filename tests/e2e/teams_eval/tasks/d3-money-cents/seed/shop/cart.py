from shop import discount, tax
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
