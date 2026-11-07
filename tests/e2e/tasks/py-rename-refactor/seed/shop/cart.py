"""A shopping cart."""
from shop.pricing import calc_total


class Cart:
    def __init__(self, tax_rate_percent):
        self.tax_rate_percent = tax_rate_percent
        self.items = []

    def add(self, unit_cents, quantity=1):
        self.items.append((unit_cents, quantity))

    def total(self):
        return calc_total(self.items, self.tax_rate_percent)
