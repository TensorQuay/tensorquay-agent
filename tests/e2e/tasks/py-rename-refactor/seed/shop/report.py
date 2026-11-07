"""Daily sales report."""
from shop.pricing import calc_total


def daily_total(orders, tax_rate_percent):
    """Return the day's total in cents over orders of (unit_cents, quantity) items."""
    return sum(calc_total(order, tax_rate_percent) for order in orders)
