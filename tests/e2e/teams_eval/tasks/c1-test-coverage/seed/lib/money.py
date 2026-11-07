"""Money amounts in cents."""
from decimal import ROUND_HALF_UP, Decimal


def parse(text):
    """'12.34', '12' or '-0.5' as cents; ValueError for anything else."""
    text = text.strip().replace(",", "")
    if not text:
        raise ValueError("empty amount")
    negative = text.startswith("-")
    if negative:
        text = text[1:]
    if text.count(".") > 1 or not text.replace(".", "").isdigit():
        raise ValueError(f"not an amount: {text!r}")
    cents = int((Decimal(text) * 100).quantize(Decimal(1), ROUND_HALF_UP))
    return -cents if negative else cents


def format_cents(cents, symbol="EUR"):
    sign = "-" if cents < 0 else ""
    cents = abs(cents)
    return f"{sign}{cents // 100:,}.{cents % 100:02d} {symbol}"


def split(cents, parts):
    """Shares of an amount that differ by at most a cent, the larger first."""
    if parts <= 0:
        raise ValueError("parts must be positive")
    base, rest = divmod(cents, parts)
    return [base + 1 if index < rest else base for index in range(parts)]


def add_vat(cents, rate=Decimal("0.2")):
    return int((Decimal(cents) * (1 + rate)).quantize(Decimal(1), ROUND_HALF_UP))
