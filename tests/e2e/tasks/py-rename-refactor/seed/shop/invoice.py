"""Invoice text."""
from shop import pricing


def invoice(lines, tax_rate_percent):
    """Return the invoice for (name, unit_cents, quantity) lines."""
    rows = [f"{name}: {quantity} x {unit / 100:.2f}" for name, unit, quantity in lines]
    total = pricing.calc_total([(unit, quantity) for _, unit, quantity in lines], tax_rate_percent)
    rows.append(f"Total with {tax_rate_percent}% tax: {total / 100:.2f}")
    return "\n".join(rows)
