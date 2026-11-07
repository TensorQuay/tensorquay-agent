from shop.money import Money
from shop.util import round2


def percent_off(price, percent):
    return Money(round2(price.amount * (100 - percent) / 100), price.currency)


def amount_off(price, off):
    return price - off if off < price else Money.zero(price.currency)
