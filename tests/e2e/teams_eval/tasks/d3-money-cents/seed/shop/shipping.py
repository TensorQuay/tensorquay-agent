from shop.money import Money

FREE_FROM = Money("50.00")


def shipping(gross):
    return Money.zero(gross.currency) if not gross < FREE_FROM else Money("4.95", gross.currency)
