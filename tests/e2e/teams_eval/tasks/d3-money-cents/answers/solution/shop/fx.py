from decimal import Decimal

from shop.money import Money

RATES = {("EUR", "USD"): Decimal("1.0865"), ("USD", "EUR"): Decimal("0.9204"), ("EUR", "GBP"): Decimal("0.8412")}


def convert(money, currency):
    if currency == money.currency:
        return money
    return Money(money.amount * RATES[(money.currency, currency)], currency)
