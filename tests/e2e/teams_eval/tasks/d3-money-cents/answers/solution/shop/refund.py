from decimal import Decimal


def partial(paid, fraction):
    return paid * Decimal(str(fraction))


def split_refund(paid, parts):
    return paid.allocate([1] * parts)
