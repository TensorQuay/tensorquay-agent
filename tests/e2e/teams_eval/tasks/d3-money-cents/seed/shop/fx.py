from shop.money import Money
from shop.util import round2

RATES = {("EUR", "USD"): 1.0865, ("USD", "EUR"): 0.9204, ("EUR", "GBP"): 0.8412}


def convert(money, currency):
    if currency == money.currency:
        return money
    return Money(round2(money.amount * RATES[(money.currency, currency)]), currency)
