from shop.money import Money


def round2(value):
    return round(value, 2)


def total(moneys, currency="EUR"):
    result = Money.zero(currency)
    for money in moneys:
        result = result + money
    return result
