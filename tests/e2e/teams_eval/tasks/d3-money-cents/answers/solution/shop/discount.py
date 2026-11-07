from decimal import Decimal

from shop.money import Money


def percent_off(price, percent):
    return price * ((Decimal(100) - Decimal(str(percent))) / 100)


def amount_off(price, off):
    return price - off if off < price else Money.zero(price.currency)
