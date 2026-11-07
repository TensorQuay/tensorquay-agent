from shop.money import Money


def total(moneys, currency="EUR"):
    result = Money.zero(currency)
    for money in moneys:
        result = result + money
    return result
