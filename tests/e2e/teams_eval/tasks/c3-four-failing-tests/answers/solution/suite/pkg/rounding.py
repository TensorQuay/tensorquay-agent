"""Rounding for invoices."""
from decimal import ROUND_HALF_UP, Decimal


def round_half_up(value, places=0):
    """A decimal string rounded half away from zero."""
    exponent = Decimal(1).scaleb(-places)
    return Decimal(value).quantize(exponent, ROUND_HALF_UP)
