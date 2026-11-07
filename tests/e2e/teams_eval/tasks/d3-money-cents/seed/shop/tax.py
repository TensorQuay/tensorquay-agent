from shop.money import Money
from shop.util import round2

RATES = {"standard": 0.19, "reduced": 0.07, "none": 0.0}


def tax(net, kind="standard"):
    return Money(round2(net.amount * RATES[kind]), net.currency)


def gross(net, kind="standard"):
    return net + tax(net, kind)
