from shop.money import Money
from shop.util import round2


def partial(paid, fraction):
    return Money(round2(paid.amount * fraction), paid.currency)


def split_refund(paid, parts):
    return paid.allocate([1] * parts)
