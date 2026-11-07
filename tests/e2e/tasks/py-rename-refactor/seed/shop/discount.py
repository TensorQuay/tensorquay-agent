"""Discounts applied before tax."""
from shop.pricing import calc_total


def discounted_total(items, tax_rate_percent, percent_off):
    """Return the total in cents after taking `percent_off` from every unit price."""
    discounted = [(unit * (100 - percent_off) // 100, quantity) for unit, quantity in items]
    return calc_total(discounted, tax_rate_percent)
