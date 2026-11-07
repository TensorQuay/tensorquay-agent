"""Price arithmetic in integer cents."""


def calc_total(items, tax_rate_percent):
    """Return the total in cents of (unit_cents, quantity) items, with tax rounded half up."""
    subtotal = sum(unit * quantity for unit, quantity in items)
    return subtotal + (subtotal * tax_rate_percent + 50) // 100
