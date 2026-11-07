from decimal import ROUND_HALF_UP, Decimal


def average(moneys):
    return (sum((money.amount for money in moneys), Decimal(0)) / len(moneys)).quantize(Decimal("0.01"),
                                                                                         rounding=ROUND_HALF_UP)


def line(name, money):
    return f"{name:<12}{money.amount:>10.2f} {money.currency}"
