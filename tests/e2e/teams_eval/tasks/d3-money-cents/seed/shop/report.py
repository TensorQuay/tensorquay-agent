from shop.util import round2


def average(moneys):
    return round2(sum(money.amount for money in moneys) / len(moneys))


def line(name, money):
    return f"{name:<12}{money.amount:>10.2f} {money.currency}"
