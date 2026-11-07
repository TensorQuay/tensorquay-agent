from decimal import Decimal

RATES = {"standard": Decimal("0.19"), "reduced": Decimal("0.07"), "none": Decimal("0")}


def tax(net, kind="standard"):
    return net * RATES[kind]


def gross(net, kind="standard"):
    return net + tax(net, kind)
